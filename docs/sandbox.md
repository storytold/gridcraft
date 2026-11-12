# The sandbox for helper processes

> **Last updated:** 2026-11-07 · **Change:** minor (no file changes by any route on Linux: Landlock's truncation right is required, and seccomp refuses what Landlock has no right for; earlier: more is refused on Linux: other processes, System V and POSIX queues, the terminal; earlier: first version: the `sandbox` crate and `gridcraft-cli sandbox-check`)

The sandbox is the layer GridCraft is to run code it doesn't trust in, such as a workbook's
Python cells: a separate process that confines itself before the code starts. Here only
`gridcraft-cli sandbox-check` starts it, and Python cells don't run ([architecture.md](architecture.md),
Python cells). The crate is `crates/sandbox`, layer 0.
This page describes what the confinement denies, how it is
checked, and where it is available.

## Threat model

The code is a program GridCraft doesn't trust, which may come from anyone. It runs with the person's
user account. Without a sandbox it could read and change the person's files, use the network and start
other programs. The sandbox's job is that it can do none of these:

- **Files:** it can read only the paths its policy lists and write no file except `/dev/null`. Nor can it change
  a file any other way: not its length, permissions, owner, extended attributes, times or flags, even
  one it can read. The exception is a standard stream the parent made a file (see below). The home
  directory, documents and other workbooks are out of reach. So `tempfile` finds no folder it can use.
- **Programs:** it can execute only the programs its policy lists, and it can't start a new
  process at all, so even those only replace itself.
- **Network:** no sockets, no connections and no listeners. The exception is a standard stream the
  parent made a socket (see below).
- **Other processes and the system:** no debugging or reading another process, no System V shared
  memory, message queues or semaphores, no POSIX message queues, no signals to another process — on Linux not even indirectly, as the `fcntl` commands that have the kernel send one are refused — no typing into a terminal it inherited, no mounts or namespaces, no kernel
  modules, BPF, keyrings or performance counters. On Linux it also can't change another process's
  limits, priority or scheduling.

Data reaches the code only through its command-line arguments (which the parent chooses), its
standard input and output (which the parent holds), and the files its policy lets it read. Nothing
else is open: the launcher closes every descriptor it inherited, because the confinement doesn't
revoke what is already open. The three standard streams stay as the parent made them, with the rights
they were opened with: a stream that is a file can be written and truncated, and one that is a
socket can still send, since neither is a file or socket the program opened. A parent gives the
program pipes. `sandbox-check` passes its own streams on, so its output sent to a file is such a
stream. The
program inherits no environment: the launcher clears it and keeps only the locale and time zone
(`LANG`, `LC_ALL`, `LC_CTYPE`, `TZ`), with `PATH` set to `/usr/bin:/bin`, so tokens and credentials
in the parent's environment don't reach it.

The sandbox doesn't limit time, memory or output size; whoever starts the program owns those limits.
The files it may read can be read, and the code can use the CPU while it runs.

## How a launch works

1. The parent builds a `Policy` (read paths and executable paths), creates a `Canary` (a fresh file
   in the temporary directory, outside every allowed path) and spawns `launch_command`. That runs
   the application's own executable again, with `--sandbox-launch` and the policy.
2. The application hands its arguments to `gridcraft_sandbox::handle_args` before anything else,
   so a launcher run opens no window and writes no log.
3. The launcher closes every open descriptor besides standard input, output and error, and refuses
   if one stays open. A desktop app's system libraries (Metal's shader cache on macOS) leave files
   open without close-on-exec, so these reach every child. `nix` holds the system call.
4. The launcher confines itself (Linux) or runs itself again under `sandbox-exec` with the profile
   (macOS, `--sandbox-confined`). On macOS the descriptors are closed first because, under a
   deny-by-default profile, the system's libraries hold a close-on-exec socket of their own, which
   must not be taken for an inherited one.
5. **The enforcement check.** Before the program runs, each of these must be refused by the
   system with a permission error:
   - reading the canary;
   - changing its permissions, to those it has, and truncating it, to the length it has, so nothing
     changes if the attempt works;
   - creating a file next to it, and in the temporary directory;
   - a TCP connection to the loopback address;
   - starting a process (a copy of itself that would exit at once);
   - signal 0 to the launcher's parent, which delivers nothing. Before it confines itself, the
     launcher checks that the same signal is allowed, and refuses otherwise: a parent that refuses
     anyone (PID 1, another user's process) would prove nothing. It refuses too if its parent has
     changed since.
   An attempt that works, or fails for another reason (a missing file, a refused connection),
   doesn't count as a denial.
6. Only then does the launcher replace itself with the program (`exec`), which inherits the
   confinement. When any step fails, the program never runs: the launcher prints
   `gridcraft-sandbox: refused: <reason>` on standard error and exits with code 97.

## Linux

The launcher applies two layers, then runs the check.

- **Landlock** (ABI 3, which is Linux 6.2 and later, enabled by default on current distributions).
  - **Filesystem rights:** every right Landlock handles is denied except:
    - reading files and listing directories under the read paths;
    - executing and reading the executable paths;
    - reading and writing `/dev/null`.
  - **Required and best-effort rights:** the filesystem rights up to ABI 3 are a hard requirement,
    so without Landlock, or with an older one, the launch is refused. ABI 3 is the first with a right
    for truncation: before it, `truncate` and `open` with `O_TRUNC` empty a file whatever the rules
    say. seccomp can't take its place, since `openat2` hides its flags from a filter, and refusing
    `O_TRUNC` would also refuse opening `/dev/null` to write. Newer rights are added where the kernel
    has them:
    - TCP bind and connect, as a second network layer (ABI 4);
    - device ioctls (ABI 5);
    - signals and abstract Unix sockets outside the sandbox (ABI 6).
  - **`no_new_privs`** is set, so no program can gain privileges.
- **seccomp**, three filters.
  - **`clone3`** fails with `ENOSYS`. The C library then uses `clone`, whose flags the filter can
    read.
  - **Refused with `EPERM`:**
    - `clone` without `CLONE_THREAD` (a new process; threads still work), plus `fork` and
      `vfork`;
    - `socket` of every family (a Unix datagram socket can send without `connect`), and
      `socketpair`, `connect`, `bind`, `listen`, `accept` and `accept4`;
    - io_uring (it runs operations outside the filter);
    - `ptrace`, `process_vm_readv`/`writev`, `pidfd_getfd`, `pidfd_open`, `pidfd_send_signal`,
      `kcmp`, `get_robust_list`, `migrate_pages`, `move_pages`;
    - System V shared memory, message queues and semaphores (`shmget`, `shmat`, `shmctl`, `shmdt`,
      `msgget`, `msgsnd`, `msgrcv`, `msgctl`, `semget`, `semop`, `semctl`, `semtimedop`), and POSIX
      message queues (`mq_open`, `mq_unlink`, `mq_timedsend`, `mq_timedreceive`, `mq_notify`,
      `mq_getsetattr`), since no path rule covers them;
    - signals to any process but the program itself, whose process ID `exec` keeps (`kill`,
      `tkill`, `tgkill`, `rt_sigqueueinfo`, `rt_tgsigqueueinfo`), so this holds on a kernel without
      Landlock's signal scope;
    - changes to any process but the program itself (`prlimit64`, `setpriority`, `ioprio_set`,
      `sched_setaffinity`, `sched_setscheduler`, `sched_setparam`, `sched_setattr`);
    - what changes a file where Landlock has no right for it, by path or by descriptor: permissions
      (`chmod`, `fchmod`, `fchmodat`, `fchmodat2`), owner (`chown`, `fchown`, `lchown`, `fchownat`),
      extended attributes (`setxattr`, `lsetxattr`, `fsetxattr`, `setxattrat`, `removexattr`,
      `lremovexattr`, `fremovexattr`, `removexattrat`), times (`utime`, `utimes`, `futimesat`,
      `utimensat`) and flags (`file_setattr`);
    - `ioctl` with `TIOCSTI` or `TIOCLINUX`, which type into a terminal the program inherited, as
      when `sandbox-check` runs a program from a shell;
    - any `ioctl` whose number says it hands the kernel data, such as `FS_IOC_SETFLAGS` and
      `FS_IOC_FSSETXATTR`, which change a file's flags on a descriptor opened only to read, and
      `FS_IOC_SET_ENCRYPTION_POLICY`, whose number says it only reads. Those that ask, such as
      `isatty`'s, and those that set a descriptor's own flags, such as `FIONBIO`, still work;
    - mounts, namespaces and file handles (`mount`, `umount2`, `pivot_root`, `chroot`,
      `unshare`, `setns`, `open_by_handle_at`, `name_to_handle_at`, the new mount API);
    - `bpf`, `perf_event_open`, `userfaultfd`, keyrings, kernel modules, `kexec`, swap,
      `reboot`, `syslog`, and port I/O on x86_64.
  - **x32 calls (x86_64 only)** fail with `EPERM`. They report the x86_64 architecture value with
    other numbers, so the rules above wouldn't match them.

  Filters are built for x86_64 and aarch64; other architectures are refused. A filter allows what it
  doesn't name, so a call a later kernel adds is allowed until it is named here; the newest named is
  `file_setattr` (Linux 6.17).

  Because no socket can be made, Python's `asyncio` doesn't work in the sandbox: its event loop
  needs a local socket pair.

## macOS

The launcher runs itself again under `sandbox-exec` with a profile built from the policy:
- `(deny default)`;
- the base system profile (`system.sb`: the system libraries, the dynamic loader's cache and a few
  system services a process needs to start);
- `process-exec` for the executable paths;
- `file-read*` for those and the read paths;
- file metadata (for path resolution), `sysctl` reads, and writing `/dev/null`.

There is no rule for `process-fork`, the network or writing any file but `/dev/null`, so those stay
denied.
A local socket pair isn't refused on macOS, so `asyncio`'s event loop starts there; it still can't
reach the network.
Paths are quoted as profile strings, and a path that can't be quoted safely is refused.

`sandbox-exec` is deprecated by Apple; support needs `/usr/bin/sandbox-exec`, and without it
`support()` reports the system as unsupported. The check above is what proves the profile holds on a
given machine.

A framework build of Python (Homebrew, python.org) starts its real interpreter,
`Python.app/Contents/MacOS/Python`, as a new process from `bin/python3`. Inside the sandbox that
start is refused, so callers run the interpreter itself. Homebrew's Python also loads `_ssl` and
`_sqlite3`'s libraries from `opt` and `Cellar`, so a policy for it needs those folders; Homebrew's
`etc` and `var`, which can hold a person's data, stay out of it.

## Other systems

Windows, FreeBSD and the web build have no confinement here: `support()` reports it and
`launch_command` refuses, so code that needs the sandbox doesn't run there (Python cells show
`#BLOCKED!`).

## Checking a machine

```text
gridcraft-cli sandbox-check
gridcraft-cli sandbox-check --read /usr -- /usr/bin/python3 -I -c "print(1)"
```

Without a program it runs a confined copy of itself that tries each denied action, plus UDP and
listing the home directory, and two that must still work (a thread, reading an allowed file). It
prints one line per attempt. With a program it runs that program confined. Exit status 0 means
everything is as required, and 1 not, including a launch the launcher refused (for example on a
Linux kernel without Landlock, or older than 6.2). 2 means GridCraft has no sandbox for this kind of system at all
(Windows, FreeBSD, an architecture other than x86_64 or aarch64, macOS without `sandbox-exec`).
