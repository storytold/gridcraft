# The sandbox for helper processes

> **Last updated:** 2026-11-06 · **Change:** minor (more is refused on Linux: other processes, System V and POSIX queues, the terminal; earlier: the app turns Python on, so cells run through the sandbox; earlier: the engine runs Python cells through the sandbox; earlier: first version: the `sandbox` crate and `gridcraft-cli sandbox-check`)

The sandbox is the layer GridCraft is to run code it doesn't trust in, such as a workbook's
Python cells: a separate process that confines itself before the code starts. The engine runs
Python cells through it, in the app, once a person turns Python on ([python.md](python.md),
[architecture.md](architecture.md), Running Python cells). The crate is `crates/sandbox`, layer 0.
This page describes what the confinement denies, how it is
checked, and where it is available.

## Threat model

The code comes from a workbook, which may come from anyone. It runs in the person's own Python
(an interpreter GridCraft found or was given, never one it downloads), with the person's user
account. Without a sandbox it could read and change the person's files, use the network and start
other programs. The sandbox's job is that it can do none of these:

- **Files:** it can read only the paths its policy lists (the interpreter's installation, the
  system's shared libraries and font folders) and write no file except `/dev/null`. The home directory,
  documents and other workbooks are out of reach. So `tempfile` finds no folder it can use. matplotlib,
  which asks for a writable folder, is given a path that doesn't exist for that one call: it can't save
  its font list there, keeps it in memory, and nothing is written. A plot comes back as bytes in the
  worker's reply.
- **Programs:** it can execute only the programs its policy lists, and it can't start a new
  process at all, so even those only replace itself.
- **Network:** no sockets, no connections and no listeners.
- **Other processes and the system:** no debugging or reading another process, no System V shared
  memory, message queues or semaphores, no POSIX message queues, no signals to another process, no typing into a terminal it inherited, no mounts or namespaces, no kernel
  modules, BPF, keyrings or performance counters. On Linux it also can't change another process's
  limits, priority or scheduling.

Data reaches the code only through its command-line arguments (which the parent chooses), its
standard input and output (which the parent holds), and the files its policy lets it read. Nothing
else is open: the launcher closes every descriptor it inherited, because the confinement doesn't
revoke what is already open. The
program inherits no environment: the launcher clears it and keeps only the locale and time zone
(`LANG`, `LC_ALL`, `LC_CTYPE`, `TZ`), with `PATH` set to `/usr/bin:/bin`, so tokens and credentials
in the parent's environment don't reach it.

The sandbox doesn't limit time, memory or output size; the client that talks to the worker owns
those limits. The files it may read (the interpreter's installation) can be read, and the code can
use the CPU while it runs.

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

- **Landlock** (Linux 5.13 and later, enabled by default on current distributions).
  - **Filesystem rights:** every right Landlock handles is denied except:
    - reading files and listing directories under the read paths;
    - executing and reading the executable paths;
    - reading and writing `/dev/null`.
  - **Required and best-effort rights:** the first ABI's filesystem rights are a hard
    requirement, so without Landlock the launch is refused. Newer rights are added where the kernel
    has them:
    - renaming across directories (ABI 2);
    - truncation (ABI 3);
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
    - `ioctl` with `TIOCSTI` or `TIOCLINUX`, which type into a terminal the program inherited, as
      when `sandbox-check` runs a program from a shell;
    - mounts, namespaces and file handles (`mount`, `umount2`, `pivot_root`, `chroot`,
      `unshare`, `setns`, `open_by_handle_at`, `name_to_handle_at`, the new mount API);
    - `bpf`, `perf_event_open`, `userfaultfd`, keyrings, kernel modules, `kexec`, swap,
      `reboot`, `syslog`, and port I/O on x86_64.
  - **x32 calls (x86_64 only)** fail with `EPERM`. They report the x86_64 architecture value with
    other numbers, so the rules above wouldn't match them.

  Filters are built for x86_64 and aarch64; other architectures are refused.

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
`_sqlite3`'s libraries from `opt` and `Cellar`, so `gridcraft_python::describe` adds those folders to
the policy; Homebrew's `etc` and `var`, which can hold a person's data, stay out.

## Other systems

Windows, FreeBSD and the web build have no confinement here: `support()` reports it and
`launch_command` refuses, so code that needs the sandbox doesn't run there (Python cells show
`#BLOCKED!`).

## Checking a machine

```text
gridcraft-cli sandbox-check
gridcraft-cli sandbox-check --read /usr -- /usr/bin/python3 -I -c "print(1)"
gridcraft-cli python-check --code "1 + 1"
```

Without a program it runs a confined copy of itself that tries each denied action, plus UDP and
listing the home directory, and two that must still work (a thread, reading an allowed file). It
prints one line per attempt. With a program it runs that program confined. Exit status 0 means
everything is as required, and 1 not, including a launch the launcher refused (for example on a
Linux kernel without Landlock). 2 means GridCraft has no sandbox for this kind of system at all
(Windows, FreeBSD, an architecture other than x86_64 or aarch64, macOS without `sandbox-exec`).

`python-check` goes further, through the same worker as a Python cell: it prints the Python found,
what runs, what it may read and the packages it can import, listed from inside the sandbox, and
runs each `--code` from its own command line.
