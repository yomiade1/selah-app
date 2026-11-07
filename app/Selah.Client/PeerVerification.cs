using System.IO.Pipes;
using System.Net.Sockets;
using System.Runtime.Versioning;
using System.Security.Principal;

namespace Selah.Client;

/// <summary>
/// Checks that the server on the other end runs as the current user (SEC-02). On Windows the
/// pipe's owner must be the current user's SID; on Unix the socket peer's uid must be ours.
/// </summary>
public static class PeerVerification
{
    [SupportedOSPlatform("windows")]
    public static void VerifyPipeServer(NamedPipeClientStream pipe)
    {
        using var identity = WindowsIdentity.GetCurrent();
        var user = identity.User ?? throw new UnauthorizedAccessException("The current user has no SID.");
        // Objects made by an elevated process are owned by the Administrators group (the token's
        // default owner) rather than the user, so accept either: both are "made by us".
        var defaultOwner = identity.Owner;
        var owner = pipe.GetAccessControl().GetOwner(typeof(SecurityIdentifier)) as SecurityIdentifier;
        if (owner is null || !(owner.Equals(user) || (defaultOwner is not null && owner.Equals(defaultOwner))))
        {
            throw new UnauthorizedAccessException("The Selah pipe belongs to another user.");
        }
    }

    public static void VerifySocketPeer(Socket socket)
    {
        uint peerUid;
        if (OperatingSystem.IsLinux())
        {
            // SOL_SOCKET = 1, SO_PEERCRED = 17: struct ucred { pid_t pid; uid_t uid; gid_t gid; }
            var buffer = new byte[12];
            socket.GetRawSocketOption(1, 17, buffer);
            peerUid = BitConverter.ToUInt32(buffer, 4);
        }
        else if (OperatingSystem.IsMacOS())
        {
            // SOL_LOCAL = 0, LOCAL_PEERCRED = 1: struct xucred { u_int version; uid_t uid; … }
            var buffer = new byte[76];
            socket.GetRawSocketOption(0, 1, buffer);
            peerUid = BitConverter.ToUInt32(buffer, 4);
        }
        else
        {
            throw new PlatformNotSupportedException("Unix socket peer checks need Linux or macOS.");
        }

        if (peerUid != NativeMethods.CurrentUid())
        {
            throw new UnauthorizedAccessException("The Selah socket belongs to another user.");
        }
    }
}
