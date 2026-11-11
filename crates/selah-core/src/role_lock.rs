//! The Simple-mode role lock (M4.5, SEC-14, SAFE-01).
//!
//! While the lock is on, core treats the person at the UI as a volunteer: Simple-mode commands
//! work, Producer and library commands are refused and logged. The state is core's own (a
//! policy setting that survives restarts), never something the client claims about itself.
//! Unlocking needs the PIN. Core keeps only a salted BLAKE3 hash of it, in [`SecretStore`], and
//! waits 30 seconds after 5 wrong tries.
//!
//! Threat model: this stops accidental and casual changes at the booth. It is not a defence
//! against someone who can already act as the same OS user (they could edit the keychain), so
//! a missing PIN record unlocks rather than leaving the church stuck in Simple mode.

use std::time::{Duration, Instant};

use selah_secrets::SecretStore;

/// The policy setting that holds the lock. Only `SetRoleLock` may change it.
pub const POLICY_KEY: &str = "live.role-lock";
/// Where the salted PIN hash lives.
pub const SECRET_KEY: &str = "live.role-lock-pin";
pub const MAX_TRIES: u32 = 5;
pub const WAIT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleLockError {
    /// The PIN must be 4 to 8 digits.
    BadPin,
    WrongPin,
    TooManyTries,
    /// The keychain refused to save the PIN.
    Storage,
}

impl RoleLockError {
    pub fn code(self) -> &'static str {
        match self {
            Self::WrongPin => "SEL-SEC-0002",
            Self::TooManyTries => "SEL-SEC-0003",
            Self::BadPin | Self::Storage => "SEL-SEC-0004",
        }
    }

    pub fn message_key(self) -> &'static str {
        match self {
            Self::WrongPin => "error-role-lock-wrong-pin",
            Self::TooManyTries => "error-role-lock-too-many-tries",
            Self::BadPin | Self::Storage => "error-role-lock-pin-format",
        }
    }
}

#[derive(Debug, Default)]
pub struct RoleLock {
    locked: bool,
    failures: u32,
    retry_after: Option<Instant>,
}

impl RoleLock {
    pub fn new(locked: bool) -> Self {
        Self {
            locked,
            ..Self::default()
        }
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    pub fn valid_pin(pin: &str) -> bool {
        (4..=8).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
    }

    fn digest(salt: &str, pin: &str) -> blake3::Hash {
        blake3::hash(format!("selah role lock v1:{salt}:{pin}").as_bytes())
    }

    pub fn lock(&mut self, secrets: &SecretStore, pin: &str) -> Result<(), RoleLockError> {
        if self.locked {
            return Ok(());
        }
        if !Self::valid_pin(pin) {
            return Err(RoleLockError::BadPin);
        }
        let salt = uuid::Uuid::new_v4().simple().to_string();
        let record = format!("{salt}:{}", Self::digest(&salt, pin).to_hex());
        secrets
            .set(SECRET_KEY, &record)
            .map_err(|_| RoleLockError::Storage)?;
        self.locked = true;
        self.failures = 0;
        self.retry_after = None;
        tracing::info!("role lock on");
        Ok(())
    }

    pub fn unlock(
        &mut self,
        secrets: &SecretStore,
        pin: &str,
        now: Instant,
    ) -> Result<(), RoleLockError> {
        if !self.locked {
            return Ok(());
        }
        if let Some(until) = self.retry_after {
            if now < until {
                tracing::warn!("role lock unlock refused: waiting after too many tries");
                return Err(RoleLockError::TooManyTries);
            }
            self.retry_after = None;
        }
        let record = secrets.get(SECRET_KEY).ok().flatten();
        let parsed = record.as_deref().and_then(|r| {
            let (salt, hex) = r.split_once(':')?;
            Some((salt.to_owned(), blake3::Hash::from_hex(hex).ok()?))
        });
        let Some((salt, expected)) = parsed else {
            tracing::warn!("role lock PIN record is missing or damaged; unlocking");
            self.locked = false;
            return Ok(());
        };
        // blake3::Hash compares in constant time.
        if Self::valid_pin(pin) && Self::digest(&salt, pin) == expected {
            self.locked = false;
            self.failures = 0;
            let _ = secrets.delete(SECRET_KEY);
            tracing::info!("role lock off");
            return Ok(());
        }
        self.failures += 1;
        tracing::warn!(
            failures = self.failures,
            "role lock unlock refused: wrong PIN"
        );
        if self.failures >= MAX_TRIES {
            self.failures = 0;
            self.retry_after = Some(now + WAIT);
            return Err(RoleLockError::TooManyTries);
        }
        Err(RoleLockError::WrongPin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pin_locks_and_only_the_same_pin_unlocks() {
        // sec: SEC-14
        let secrets = SecretStore::memory();
        let mut lock = RoleLock::new(false);
        assert_eq!(lock.lock(&secrets, "12"), Err(RoleLockError::BadPin));
        assert_eq!(lock.lock(&secrets, "12ab"), Err(RoleLockError::BadPin));
        assert_eq!(lock.lock(&secrets, "123456789"), Err(RoleLockError::BadPin));
        lock.lock(&secrets, "2468").unwrap();
        assert!(lock.is_locked());
        // Only a salted hash is kept, never the PIN.
        let record = secrets.get(SECRET_KEY).unwrap().unwrap();
        assert!(!record.contains("2468"));
        let now = Instant::now();
        assert_eq!(
            lock.unlock(&secrets, "1357", now),
            Err(RoleLockError::WrongPin)
        );
        assert!(lock.is_locked());
        lock.unlock(&secrets, "2468", now).unwrap();
        assert!(!lock.is_locked());
        assert_eq!(secrets.get(SECRET_KEY).unwrap(), None);
    }

    #[test]
    fn five_wrong_tries_wait_thirty_seconds_even_for_the_right_pin() {
        // sec: SEC-14
        let secrets = SecretStore::memory();
        let mut lock = RoleLock::new(false);
        lock.lock(&secrets, "2468").unwrap();
        let start = Instant::now();
        for _ in 0..MAX_TRIES - 1 {
            assert_eq!(
                lock.unlock(&secrets, "0000", start),
                Err(RoleLockError::WrongPin)
            );
        }
        assert_eq!(
            lock.unlock(&secrets, "0000", start),
            Err(RoleLockError::TooManyTries)
        );
        assert_eq!(
            lock.unlock(&secrets, "2468", start + Duration::from_secs(29)),
            Err(RoleLockError::TooManyTries)
        );
        assert!(lock.is_locked());
        lock.unlock(&secrets, "2468", start + WAIT).unwrap();
        assert!(!lock.is_locked());
    }

    #[test]
    fn a_missing_pin_record_unlocks_instead_of_trapping_the_church() {
        let secrets = SecretStore::memory();
        let mut lock = RoleLock::new(true);
        lock.unlock(&secrets, "9999", Instant::now()).unwrap();
        assert!(!lock.is_locked());
    }
}
