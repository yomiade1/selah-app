using System.Runtime.InteropServices;
using System.Text;

namespace Selah.Client;

/// <summary>The services Selah's processes expose (matches <c>selah_ipc::Service</c>).</summary>
public enum IpcService
{
    Core,
    Engine,
    Emergency,
    Watchdog,
}

/// <summary>
/// Where a service listens (ADR-004). Windows: <c>\\.\pipe\selah-{namespace}-{service}</c>.
/// Unix: <c>{runtime}/{namespace}-{service}.sock</c> in a private 0700 directory.
/// </summary>
public sealed record IpcEndpoint
{
    private IpcEndpoint(string ns, IpcService service)
    {
        Namespace = ns;
        Service = service;
    }

    public string Namespace { get; }

    public IpcService Service { get; }

    /// <summary>Endpoint for the current user; <c>SELAH_IPC_NAMESPACE</c> overrides the namespace.</summary>
    public static IpcEndpoint ForCurrentUser(IpcService service)
    {
        var ns = Environment.GetEnvironmentVariable("SELAH_IPC_NAMESPACE");
        return Create(string.IsNullOrEmpty(ns) ? Environment.UserName : ns, service);
    }

    /// <summary>The namespace is reduced to [A-Za-z0-9_-], at most 40 characters, as in Rust.</summary>
    public static IpcEndpoint Create(string ns, IpcService service)
    {
        var clean = new StringBuilder();
        foreach (var c in ns)
        {
            if (clean.Length == 40)
            {
                break;
            }

            if (char.IsAsciiLetterOrDigit(c) || c is '-' or '_')
            {
                clean.Append(c);
            }
        }

        return new IpcEndpoint(clean.Length == 0 ? "user" : clean.ToString(), service);
    }

    public IpcEndpoint Sibling(IpcService service) => new(Namespace, service);

    public string ServiceName => Service switch
    {
        IpcService.Core => "core",
        IpcService.Engine => "engine",
        IpcService.Emergency => "emergency",
        IpcService.Watchdog => "watchdog",
        _ => throw new ArgumentOutOfRangeException(nameof(Service)),
    };

    /// <summary>Pipe name without the <c>\\.\pipe\</c> prefix, as NamedPipeClientStream wants it.</summary>
    public string PipeName => $"selah-{Namespace}-{ServiceName}";

    public string SocketPath => Path.Combine(RuntimeDirectory.Path, $"{Namespace}-{ServiceName}.sock");
}

/// <summary>The private runtime directory shared with the Rust processes.</summary>
public static class RuntimeDirectory
{
    public static string Path
    {
        get
        {
            var overridden = Environment.GetEnvironmentVariable("SELAH_RUNTIME_DIR");
            if (!string.IsNullOrEmpty(overridden))
            {
                return overridden;
            }

            if (OperatingSystem.IsWindows())
            {
                return System.IO.Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Selah", "run");
            }

            var xdg = Environment.GetEnvironmentVariable("XDG_RUNTIME_DIR");
            return string.IsNullOrEmpty(xdg)
                ? $"/tmp/selah-{NativeMethods.getuid()}"
                : System.IO.Path.Combine(xdg, "selah");
        }
    }
}

internal static partial class NativeMethods
{
    [DllImport("libc", EntryPoint = "getuid")]
#pragma warning disable SYSLIB1054 // LibraryImport needs unsafe blocks, which Selah disallows (SEC-11).
    internal static extern uint getuid();
#pragma warning restore SYSLIB1054
}
