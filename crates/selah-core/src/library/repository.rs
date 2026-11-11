use std::collections::HashSet;

use anyhow::{Context, ensure};
use prost::Message;
use rusqlite::{OptionalExtension, Transaction, params};
use selah_proto::v1::*;

use super::{Library, MAX_CHILDREN, MAX_DOCUMENT_BYTES, required_id, validate_id};

/// A closed list of tables: caller input can never become an SQL identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentKind {
    Song,
    Template,
    Media,
    Presentation,
    Service,
    ServiceType,
    Team,
    Tag,
    Look,
    Prop,
    Message,
    Timer,
    Macro,
    ClearGroup,
    StageLayout,
}

impl DocumentKind {
    pub(super) fn table(self) -> &'static str {
        match self {
            Self::Song => "songs",
            Self::Template => "templates",
            Self::Media => "media_items",
            Self::Presentation => "presentations",
            Self::Service => "services",
            Self::ServiceType => "service_types",
            Self::Team => "teams",
            Self::Tag => "tags",
            Self::Look => "looks",
            Self::Prop => "props",
            Self::Message => "messages",
            Self::Timer => "timers",
            Self::Macro => "macros",
            Self::ClearGroup => "clear_groups",
            Self::StageLayout => "stage_layouts",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Song => "song",
            Self::Template => "template",
            Self::Media => "media",
            Self::Presentation => "presentation",
            Self::Service => "service",
            Self::ServiceType => "service_type",
            Self::Team => "team",
            Self::Tag => "tag",
            Self::Look => "look",
            Self::Prop => "prop",
            Self::Message => "message",
            Self::Timer => "timer",
            Self::Macro => "macro",
            Self::ClearGroup => "clear_group",
            Self::StageLayout => "stage_layout",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "song" => Some(Self::Song),
            "template" => Some(Self::Template),
            "media" => Some(Self::Media),
            "presentation" => Some(Self::Presentation),
            "service" => Some(Self::Service),
            "service_type" => Some(Self::ServiceType),
            "team" => Some(Self::Team),
            "tag" => Some(Self::Tag),
            "look" => Some(Self::Look),
            "prop" => Some(Self::Prop),
            "message" => Some(Self::Message),
            "timer" => Some(Self::Timer),
            "macro" => Some(Self::Macro),
            "clear_group" => Some(Self::ClearGroup),
            "stage_layout" => Some(Self::StageLayout),
            _ => None,
        }
    }

    pub fn from_proto(value: i32) -> Option<Self> {
        match DocumentType::try_from(value).ok()? {
            DocumentType::Song => Some(Self::Song),
            DocumentType::Template => Some(Self::Template),
            DocumentType::Media => Some(Self::Media),
            DocumentType::Presentation => Some(Self::Presentation),
            DocumentType::Service => Some(Self::Service),
            DocumentType::ServiceType => Some(Self::ServiceType),
            DocumentType::Team => Some(Self::Team),
            DocumentType::Tag => Some(Self::Tag),
            DocumentType::Look => Some(Self::Look),
            DocumentType::Prop => Some(Self::Prop),
            DocumentType::Message => Some(Self::Message),
            DocumentType::Timer => Some(Self::Timer),
            DocumentType::Macro => Some(Self::Macro),
            DocumentType::ClearGroup => Some(Self::ClearGroup),
            DocumentType::StageLayout => Some(Self::StageLayout),
            DocumentType::Unspecified => None,
        }
    }

    pub fn to_proto(self) -> DocumentType {
        match self {
            Self::Song => DocumentType::Song,
            Self::Template => DocumentType::Template,
            Self::Media => DocumentType::Media,
            Self::Presentation => DocumentType::Presentation,
            Self::Service => DocumentType::Service,
            Self::ServiceType => DocumentType::ServiceType,
            Self::Team => DocumentType::Team,
            Self::Tag => DocumentType::Tag,
            Self::Look => DocumentType::Look,
            Self::Prop => DocumentType::Prop,
            Self::Message => DocumentType::Message,
            Self::Timer => DocumentType::Timer,
            Self::Macro => DocumentType::Macro,
            Self::ClearGroup => DocumentType::ClearGroup,
            Self::StageLayout => DocumentType::StageLayout,
        }
    }
}

mod sealed {
    pub trait Sealed {}
}

/// Typed repository contract. Implementations are sealed so table names stay trusted.
pub trait Document: Message + Default + sealed::Sealed {
    const KIND: DocumentKind;
    fn id(&self) -> &Option<Id>;
    fn name(&self) -> &str;
    fn validate(&self) -> anyhow::Result<()>;
    #[doc(hidden)]
    fn write(&self, tx: &Transaction<'_>) -> anyhow::Result<()>;
}

fn name(value: &str) -> anyhow::Result<()> {
    ensure!(
        !value.trim().is_empty() && value.chars().count() <= 512,
        "name must have 1 to 512 characters"
    );
    ensure!(!value.contains('\0'), "name contains a NUL character");
    Ok(())
}

fn unique_ids<'a>(ids: impl IntoIterator<Item = &'a Option<Id>>) -> anyhow::Result<()> {
    let mut seen = HashSet::new();
    for id in ids {
        ensure!(seen.insert(required_id(id)?.to_vec()), "duplicate child ID");
    }
    ensure!(seen.len() <= MAX_CHILDREN, "too many children");
    Ok(())
}

fn optional_id(id: &Option<Id>) -> anyhow::Result<()> {
    if let Some(id) = id {
        validate_id(id)?;
    }
    Ok(())
}

impl sealed::Sealed for Song {}
impl Document for Song {
    const KIND: DocumentKind = DocumentKind::Song;
    fn id(&self) -> &Option<Id> {
        &self.id
    }
    fn name(&self) -> &str {
        &self.title
    }
    fn validate(&self) -> anyhow::Result<()> {
        required_id(&self.id)?;
        name(&self.title)?;
        ensure!(
            self.tempo.is_finite() && (0.0..=1000.0).contains(&self.tempo),
            "invalid tempo"
        );
        unique_ids(self.sections.iter().map(|s| &s.id))?;
        unique_ids(self.arrangements.iter().map(|a| &a.id))?;
        ensure!(
            self.tags.len() <= MAX_CHILDREN && self.authors.len() <= MAX_CHILDREN,
            "too many tags or authors"
        );
        let mut section_ids = HashSet::new();
        for section in &self.sections {
            section_ids.insert(required_id(&section.id)?);
            name(&section.label)?;
            ensure!(
                SectionKind::try_from(section.kind).is_ok() && section.kind != 0,
                "unknown section kind"
            );
            ensure!(
                section.lines.len() <= MAX_CHILDREN && section.translations.len() <= 64,
                "too many lines or translations"
            );
            if let Some(color) = &section.color {
                ensure!(
                    [color.red, color.green, color.blue, color.alpha]
                        .iter()
                        .all(|c| c.is_finite() && (0.0..=1.0).contains(c)),
                    "invalid section color"
                );
            }
            for line in &section.lines {
                ensure!(line.chords.len() <= MAX_CHILDREN, "too many chords");
                let length = line.text.chars().count();
                for chord in &line.chords {
                    ensure!(
                        chord.position as usize <= length
                            && !chord.text.is_empty()
                            && chord.text.len() <= 64,
                        "invalid chord position or text"
                    );
                }
            }
            for (language, lines) in &section.translations {
                ensure!(
                    !language.is_empty()
                        && language.len() <= 35
                        && lines.lines.len() <= MAX_CHILDREN,
                    "invalid translation"
                );
            }
        }
        for arrangement in &self.arrangements {
            name(&arrangement.name)?;
            ensure!(
                arrangement.section_ids.len() <= MAX_CHILDREN,
                "arrangement is too long"
            );
            for id in &arrangement.section_ids {
                validate_id(id)?;
                ensure!(
                    section_ids.contains(id.value.as_slice()),
                    "arrangement refers to a missing section"
                );
            }
        }
        Ok(())
    }
    fn write(&self, tx: &Transaction<'_>) -> anyhow::Result<()> {
        let id = required_id(&self.id)?;
        tx.prepare_cached("INSERT INTO songs VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(id) DO UPDATE SET title=excluded.title, authors=excluded.authors, ccli_number=excluded.ccli_number, tags=excluded.tags, data=excluded.data")?
            .execute(params![id, self.title, self.authors.join("\n"), self.ccli_number, self.tags.join("\n"), self.encode_to_vec()])?;
        tx.prepare_cached("DELETE FROM song_sections WHERE song_id=?1")?
            .execute([id])?;
        tx.prepare_cached("DELETE FROM song_arrangements WHERE song_id=?1")?
            .execute([id])?;
        for (position, section) in self.sections.iter().enumerate() {
            tx.prepare_cached("INSERT INTO song_sections VALUES (?1, ?2, ?3, ?4)")?
                .execute(params![
                    required_id(&section.id)?,
                    id,
                    position as i64,
                    section.encode_to_vec()
                ])?;
        }
        for (position, arrangement) in self.arrangements.iter().enumerate() {
            tx.prepare_cached("INSERT INTO song_arrangements VALUES (?1, ?2, ?3, ?4)")?
                .execute(params![
                    required_id(&arrangement.id)?,
                    id,
                    position as i64,
                    arrangement.encode_to_vec()
                ])?;
        }
        super::search::index_song_in_tx(tx, self)?;
        Ok(())
    }
}

impl sealed::Sealed for MediaItem {}
impl Document for MediaItem {
    const KIND: DocumentKind = DocumentKind::Media;
    fn id(&self) -> &Option<Id> {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn validate(&self) -> anyhow::Result<()> {
        required_id(&self.id)?;
        name(&self.name)?;
        super::media::validate_address(&self.content_hash, &self.extension)?;
        ensure!(
            MediaKind::try_from(self.kind).is_ok() && self.kind != 0,
            "unknown media kind"
        );
        ensure!(
            self.size_bytes <= super::media::MAX_MEDIA_BYTES,
            "media is too large"
        );
        ensure!(self.tags.len() <= MAX_CHILDREN, "too many tags");
        Ok(())
    }
    fn write(&self, tx: &Transaction<'_>) -> anyhow::Result<()> {
        let id = required_id(&self.id)?;
        tx.execute("INSERT INTO media_items VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(id) DO UPDATE SET name=excluded.name, content_hash=excluded.content_hash, extension=excluded.extension, data=excluded.data", params![id, self.name, self.content_hash, self.extension, self.encode_to_vec()])?;
        tx.execute("DELETE FROM media_tags WHERE media_id=?1", [id])?;
        for tag in &self.tags {
            name(tag)?;
            tx.execute(
                "INSERT OR IGNORE INTO media_tags VALUES (?1, ?2)",
                params![id, tag],
            )?;
        }
        super::search::index_media_in_tx(tx, self)?;
        Ok(())
    }
}

// The main document and its ordered child rows are replaced in the same transaction.
macro_rules! document {
    ($type:ty, $kind:ident, $validate:expr, $children:expr) => {
        impl sealed::Sealed for $type {}
        impl Document for $type {
            const KIND: DocumentKind = DocumentKind::$kind;
            fn id(&self) -> &Option<Id> { &self.id }
            fn name(&self) -> &str { &self.name }
            fn validate(&self) -> anyhow::Result<()> {
                required_id(&self.id)?; name(&self.name)?;
                ($validate)(self)
            }
            fn write(&self, tx: &Transaction<'_>) -> anyhow::Result<()> {
                let sql = format!("INSERT INTO {} VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET name=excluded.name, data=excluded.data", Self::KIND.table());
                tx.execute(&sql, params![required_id(&self.id)?, self.name, self.encode_to_vec()])?;
                ($children)(self, tx)
            }
        }
    };
}

document!(
    Template,
    Template,
    |value: &Template| {
        ensure!(
            value.variants.len() <= selah_render::template::MAX_VARIANTS,
            "too many template variants"
        );
        unique_ids(value.variants.iter().map(|v| &v.id))?;
        for variant in &value.variants {
            ensure!((1..=100).contains(&variant.max_lines), "invalid line limit");
            let size = variant
                .resolution
                .as_ref()
                .context("missing template resolution")?;
            ensure!(
                size.width > 0 && size.height > 0 && size.width <= 16384 && size.height <= 16384,
                "invalid template resolution"
            );
            selah_render::template::TemplateStyle::from_json(&variant.style_json)?;
        }
        Ok(())
    },
    |value: &Template, tx: &Transaction<'_>| {
        let id = required_id(&value.id)?;
        tx.execute("DELETE FROM template_variants WHERE template_id=?1", [id])?;
        for (position, child) in value.variants.iter().enumerate() {
            tx.execute(
                "INSERT INTO template_variants VALUES (?1, ?2, ?3, ?4)",
                params![
                    required_id(&child.id)?,
                    id,
                    position as i64,
                    child.encode_to_vec()
                ],
            )?;
        }
        Ok(())
    }
);

document!(
    Presentation,
    Presentation,
    |value: &Presentation| {
        optional_id(&value.template_id)?;
        unique_ids(value.slides.iter().map(|s| &s.id))?;
        for slide in &value.slides {
            optional_id(&slide.media_id)?;
            if !slide.elements_json.trim().is_empty() {
                selah_render::slide_elements::SlideElementsDocument::from_json_strict(
                    &slide.elements_json,
                )?;
            }
        }
        Ok(())
    },
    |value: &Presentation, tx: &Transaction<'_>| {
        let id = required_id(&value.id)?;
        tx.execute("DELETE FROM slides WHERE presentation_id=?1", [id])?;
        for (position, child) in value.slides.iter().enumerate() {
            tx.execute(
                "INSERT INTO slides VALUES (?1, ?2, ?3, ?4)",
                params![
                    required_id(&child.id)?,
                    id,
                    position as i64,
                    child.encode_to_vec()
                ],
            )?;
        }
        Ok(())
    }
);

document!(
    Service,
    Service,
    |value: &Service| {
        optional_id(&value.service_type_id)?;
        optional_id(&value.source_service_id)?;
        optional_id(&value.default_template_id)?;
        unique_ids(value.items.iter().map(|s| &s.id))?;
        ensure!(value.team_ids.len() <= MAX_CHILDREN, "too many teams");
        for id in &value.team_ids {
            validate_id(id)?;
        }
        for item in &value.items {
            ensure!(
                ServiceItemKind::try_from(item.kind).is_ok() && item.kind != 0,
                "unknown service item kind"
            );
            optional_id(&item.content_id)?;
            optional_id(&item.arrangement_id)?;
            optional_id(&item.template_id)?;
            optional_id(&item.look_id)?;
            optional_id(&item.background_media_id)?;
        }
        Ok(())
    },
    |value: &Service, tx: &Transaction<'_>| {
        let id = required_id(&value.id)?;
        tx.execute("DELETE FROM service_items WHERE service_id=?1", [id])?;
        for (position, child) in value.items.iter().enumerate() {
            tx.execute(
                "INSERT INTO service_items VALUES (?1, ?2, ?3, ?4)",
                params![
                    required_id(&child.id)?,
                    id,
                    position as i64,
                    child.encode_to_vec()
                ],
            )?;
        }
        Ok(())
    }
);

document!(
    ServiceType,
    ServiceType,
    |value: &ServiceType| {
        optional_id(&value.default_template_id)?;
        unique_ids(value.template_items.iter().map(|s| &s.id))?;
        for item in &value.template_items {
            ensure!(
                ServiceItemKind::try_from(item.kind).is_ok() && item.kind != 0,
                "unknown service item kind"
            );
            optional_id(&item.content_id)?;
            optional_id(&item.arrangement_id)?;
            optional_id(&item.template_id)?;
            optional_id(&item.look_id)?;
            optional_id(&item.background_media_id)?;
        }
        Ok(())
    },
    |_: &ServiceType, _: &Transaction<'_>| Ok(())
);
document!(
    Team,
    Team,
    |value: &Team| {
        unique_ids(value.members.iter().map(|m| &m.id))?;
        for member in &value.members {
            ensure!(!member.name.trim().is_empty(), "team member name is empty");
        }
        Ok(())
    },
    |_: &Team, _: &Transaction<'_>| Ok(())
);
document!(
    Tag,
    Tag,
    |_: &Tag| Ok(()),
    |_: &Tag, _: &Transaction<'_>| Ok(())
);
document!(
    Look,
    Look,
    |value: &Look| {
        unique_ids(value.rules.iter().map(|r| &r.id))?;
        ensure!(value.rules.len() <= 32, "too many look rules");
        for rule in &value.rules {
            optional_id(&rule.template_override_id)?;
            ensure!(
                rule.enabled_layers.len() <= 16,
                "too many enabled layers in look rule"
            );
            for &lk in &rule.enabled_layers {
                ensure!(
                    LayerKind::try_from(lk).is_ok() && lk != 0,
                    "invalid layer kind in look rule"
                );
            }
        }
        Ok(())
    },
    |value: &Look, tx: &Transaction<'_>| {
        let id = required_id(&value.id)?;
        tx.execute("DELETE FROM look_rules WHERE look_id=?1", [id])?;
        for (position, rule) in value.rules.iter().enumerate() {
            tx.execute(
                "INSERT INTO look_rules VALUES (?1, ?2, ?3, ?4)",
                params![
                    required_id(&rule.id)?,
                    id,
                    position as i64,
                    rule.encode_to_vec()
                ],
            )?;
        }
        Ok(())
    }
);
document!(
    Prop,
    Prop,
    |value: &Prop| {
        optional_id(&value.media_id)?;
        ensure!(
            value.label.chars().count() <= 512,
            "prop label is too long (max 512 chars)"
        );
        ensure!(
            value.auto_clear_seconds <= 86_400,
            "prop auto_clear_seconds exceeds 24 hours"
        );
        Ok(())
    },
    |_: &Prop, _: &Transaction<'_>| Ok(())
);
document!(
    LiveMessage,
    Message,
    |value: &LiveMessage| {
        optional_id(&value.timer_id)?;
        ensure!(
            !value.template_text.trim().is_empty() && value.template_text.chars().count() <= 512,
            "message template_text must be 1 to 512 characters"
        );
        ensure!(
            value.default_tokens.len() <= 32,
            "too many default message tokens"
        );
        for (k, v) in &value.default_tokens {
            ensure!(
                !k.trim().is_empty() && k.len() <= 64 && v.chars().count() <= 256,
                "invalid message token key or value"
            );
        }
        ensure!(
            value.duration_seconds <= 86_400,
            "message duration exceeds 24 hours"
        );
        Ok(())
    },
    |_: &LiveMessage, _: &Transaction<'_>| Ok(())
);
document!(
    Timer,
    Timer,
    |value: &Timer| {
        ensure!(
            TimerKind::try_from(value.kind).is_ok() && value.kind != 0,
            "unknown timer kind"
        );
        ensure!(
            value.duration_ms <= 86_400_000,
            "timer duration exceeds 24 hours"
        );
        ensure!(
            value.target_time_of_day_seconds < 86_400,
            "target_time_of_day_seconds must be under 86400"
        );
        optional_id(&value.overrun_macro_id)?;
        Ok(())
    },
    |_: &Timer, _: &Transaction<'_>| Ok(())
);
document!(
    Macro,
    Macro,
    |value: &Macro| {
        unique_ids(value.actions.iter().map(|a| &a.id))?;
        ensure!(
            value.actions.len() <= 32,
            "macro has too many actions (max 32)"
        );
        for action in &value.actions {
            ensure!(
                MacroActionKind::try_from(action.kind).is_ok() && action.kind != 0,
                "unknown macro action kind"
            );
            optional_id(&action.target_id)?;
        }
        Ok(())
    },
    |value: &Macro, tx: &Transaction<'_>| {
        let id = required_id(&value.id)?;
        tx.execute("DELETE FROM macro_actions WHERE macro_id=?1", [id])?;
        for (position, action) in value.actions.iter().enumerate() {
            tx.execute(
                "INSERT INTO macro_actions VALUES (?1, ?2, ?3, ?4)",
                params![
                    required_id(&action.id)?,
                    id,
                    position as i64,
                    action.encode_to_vec()
                ],
            )?;
        }
        Ok(())
    }
);
document!(
    ClearGroup,
    ClearGroup,
    |value: &ClearGroup| {
        ensure!(
            value.layers_to_clear.len() <= 16,
            "too many layers in clear group"
        );
        for &lk in &value.layers_to_clear {
            ensure!(
                LayerKind::try_from(lk).is_ok() && lk != 0,
                "invalid layer kind in clear group"
            );
        }
        Ok(())
    },
    |_: &ClearGroup, _: &Transaction<'_>| Ok(())
);
document!(
    StageLayout,
    StageLayout,
    |value: &StageLayout| {
        unique_ids(value.elements.iter().map(|e| &e.id))?;
        crate::stage::validate_layout(value)?;
        ensure!(value.elements.len() <= 32, "too many stage layout elements");
        for el in &value.elements {
            ensure!(
                StageElementKind::try_from(el.kind).is_ok() && el.kind != 0,
                "invalid stage element kind"
            );
        }
        Ok(())
    },
    |_: &StageLayout, _: &Transaction<'_>| Ok(())
);

impl Library {
    /// Storage primitive; the command bus must authorise and log before calling this.
    pub fn save<T: Document>(&mut self, document: &T) -> anyhow::Result<()> {
        ensure!(
            document.encoded_len() <= MAX_DOCUMENT_BYTES,
            "document is too large"
        );
        document.validate()?;
        if T::KIND == DocumentKind::Media {
            let item = MediaItem::decode(document.encode_to_vec().as_slice())?;
            self.media_store()?
                .verify(&item.content_hash, &item.extension, item.size_bytes)?;
        }
        let tx = self.db.transaction()?;
        document.write(&tx)?;
        tx.commit()?;
        Ok(())
    }

    pub fn get<T: Document>(&self, id: &Id) -> anyhow::Result<Option<T>> {
        validate_id(id)?;
        let sql = format!("SELECT data FROM {} WHERE id=?1", T::KIND.table());
        let bytes: Option<Vec<u8>> = self
            .db
            .query_row(&sql, [&id.value], |r| r.get(0))
            .optional()?;
        bytes
            .map(|bytes| {
                ensure!(
                    bytes.len() <= MAX_DOCUMENT_BYTES,
                    "stored document is too large"
                );
                let value = T::decode(bytes.as_slice())?;
                value.validate()?;
                ensure!(
                    required_id(value.id())? == id.value,
                    "stored document ID mismatch"
                );
                Ok(value)
            })
            .transpose()
    }

    /// Stable order and bounded pages. The UI never loads the entire library at once.
    pub fn list<T: Document>(&self, limit: u32, offset: u32) -> anyhow::Result<Vec<T>> {
        ensure!((1..=200).contains(&limit), "page size must be 1 to 200");
        let sql = format!(
            "SELECT id FROM {} ORDER BY id LIMIT ?1 OFFSET ?2",
            T::KIND.table()
        );
        let mut statement = self.db.prepare_cached(&sql)?;
        let ids = statement
            .query_map(params![limit, offset], |r| r.get::<_, Vec<u8>>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.into_iter()
            .map(|value| self.get(&Id { value })?.context("document disappeared"))
            .collect()
    }

    pub fn delete<T: Document>(&mut self, id: &Id) -> anyhow::Result<bool> {
        validate_id(id)?;
        let tx = self.db.transaction()?;
        let changed = delete_in_tx(&tx, T::KIND, &id.value)?;
        tx.commit()?;
        Ok(changed)
    }

    /// Storage primitive for fast bulk inserts (for example benchmarks and importers).
    pub fn save_songs_batch(&mut self, songs: &[Song]) -> anyhow::Result<()> {
        for song in songs {
            ensure!(
                song.encoded_len() <= MAX_DOCUMENT_BYTES,
                "document is too large"
            );
            song.validate()?;
        }
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for song in songs {
            song.write(&tx)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn media_flash_warning_by_hash(&self, content_hash: &str) -> anyhow::Result<bool> {
        let mut stmt = self
            .db
            .prepare("SELECT data FROM media_items WHERE content_hash = ?1")?;
        let mut rows = stmt.query([content_hash])?;
        while let Some(row) = rows.next()? {
            let data: Vec<u8> = row.get(0)?;
            if let Ok(item) = MediaItem::decode(data.as_slice())
                && item.flash_warning
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

pub(super) fn read_snapshot_in_tx(
    tx: &Transaction<'_>,
    kind: DocumentKind,
    id: &[u8],
) -> anyhow::Result<Option<Vec<u8>>> {
    let sql = format!("SELECT data FROM {} WHERE id=?1", kind.table());
    Ok(tx.query_row(&sql, [id], |r| r.get(0)).optional()?)
}

pub(super) fn delete_in_tx(
    tx: &Transaction<'_>,
    kind: DocumentKind,
    id: &[u8],
) -> anyhow::Result<bool> {
    match kind {
        DocumentKind::Song => super::search::remove_song_from_index_in_tx(tx, id)?,
        DocumentKind::Media => super::search::remove_media_from_index_in_tx(tx, id)?,
        _ => {}
    }
    let sql = format!("DELETE FROM {} WHERE id=?1", kind.table());
    Ok(tx.execute(&sql, [id])? != 0)
}

fn verify_and_write<T: Document>(
    media_root: &std::path::Path,
    tx: &Transaction<'_>,
    expected_id: &[u8],
    data: &[u8],
) -> anyhow::Result<()> {
    ensure!(data.len() <= MAX_DOCUMENT_BYTES, "document is too large");
    let doc = T::decode(data)?;
    ensure!(
        required_id(doc.id())? == expected_id,
        "stored document ID mismatch"
    );
    doc.validate()?;
    if T::KIND == DocumentKind::Media {
        let item = MediaItem::decode(data)?;
        super::MediaStore::open(media_root)?.verify(
            &item.content_hash,
            &item.extension,
            item.size_bytes,
        )?;
    }
    doc.write(tx)?;
    Ok(())
}

pub(super) fn apply_snapshot_in_tx(
    media_root: &std::path::Path,
    tx: &Transaction<'_>,
    kind: DocumentKind,
    id: &[u8],
    present: bool,
    data: &[u8],
) -> anyhow::Result<()> {
    if !present {
        delete_in_tx(tx, kind, id)?;
        return Ok(());
    }
    match kind {
        DocumentKind::Song => verify_and_write::<Song>(media_root, tx, id, data),
        DocumentKind::Template => verify_and_write::<Template>(media_root, tx, id, data),
        DocumentKind::Media => verify_and_write::<MediaItem>(media_root, tx, id, data),
        DocumentKind::Presentation => verify_and_write::<Presentation>(media_root, tx, id, data),
        DocumentKind::Service => verify_and_write::<Service>(media_root, tx, id, data),
        DocumentKind::ServiceType => verify_and_write::<ServiceType>(media_root, tx, id, data),
        DocumentKind::Team => verify_and_write::<Team>(media_root, tx, id, data),
        DocumentKind::Tag => verify_and_write::<Tag>(media_root, tx, id, data),
        DocumentKind::Look => verify_and_write::<Look>(media_root, tx, id, data),
        DocumentKind::Prop => verify_and_write::<Prop>(media_root, tx, id, data),
        DocumentKind::Message => verify_and_write::<LiveMessage>(media_root, tx, id, data),
        DocumentKind::Timer => verify_and_write::<Timer>(media_root, tx, id, data),
        DocumentKind::Macro => verify_and_write::<Macro>(media_root, tx, id, data),
        DocumentKind::ClearGroup => verify_and_write::<ClearGroup>(media_root, tx, id, data),
        DocumentKind::StageLayout => verify_and_write::<StageLayout>(media_root, tx, id, data),
    }
}
