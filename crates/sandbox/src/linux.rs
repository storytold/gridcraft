//! Linux: the launcher confines itself with Landlock, then seccomp, then `exec`s the program,
//! which inherits both.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use landlock::{
    ABI, Access, AccessFs, AccessNet, CompatLevel, Compatible, CreateRulesetError, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetError,
    RulesetStatus, Scope, path_beneath_rules,
};
use seccompiler::{BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter, SeccompRule, TargetArch};

use crate::policy::Checked;
use crate::{Result, SandboxError, Support};

/// The newest Landlock ABI this crate knows. Rights it adds are requested on a best-effort basis;
/// the first ABI's filesystem rights are required.
const NEWEST: ABI = ABI::V9;

pub fn support() -> Support {
    match arch() {
        Some(_) => Support::Available("Landlock and seccomp (checked again at each launch)".into()),
        None => Support::Unsupported(format!("seccomp filters are built for x86_64 and aarch64, not {}", std::env::consts::ARCH)),
    }
}

/// Linux runs the launcher directly: it confines itself.
pub fn command(_: &Checked, exe: &Path, launch: &[OsString]) -> Result<Command> {
    arch().ok_or_else(|| SandboxError::Unsupported(format!("no seccomp filter for {}", std::env::consts::ARCH)))?;
    let mut c = Command::new(exe);
    c.args(launch);
    Ok(c)
}

pub fn confine(policy: &Checked, _: u32, _: &[OsString]) -> Result<()> {
    landlock(policy)?;
    seccomp()
}

pub fn exec(program: &Path, args: &[OsString]) -> Result<std::convert::Infallible> {
    crate::exec_unix(program, args)
}

fn not_enforced(e: impl std::fmt::Display) -> SandboxError {
    SandboxError::NotEnforced(e.to_string())
}

/// A kernel without Landlock, or with it off, in the Landlock crate's own text. Only that case says
/// this, since the tests that skip on such a machine look for it: any other failure, such as too many
/// layers or a refused ruleset, comes from a machine that has Landlock, which must not be taken for
/// one that doesn't.
fn landlock_not_enforced(e: impl std::fmt::Display) -> SandboxError {
    SandboxError::NotEnforced(format!("Landlock is not enforced: {e}"))
}

/// What a failed `landlock_create_ruleset` says about the kernel: `ENOSYS` is one built without
/// Landlock and `EOPNOTSUPP` one with it off at boot. Anything else, such as `EINVAL` or too many open
/// files, comes from a kernel that has it.
fn says_no_landlock(errno: Option<i32>) -> bool {
    matches!(errno, Some(libc::ENOSYS | libc::EOPNOTSUPP))
}

fn ruleset_error(e: RulesetError) -> SandboxError {
    let absent =
        matches!(&e, RulesetError::CreateRuleset(CreateRulesetError::CreateRulesetCall { source, .. }) if says_no_landlock(source.raw_os_error()));
    if absent { landlock_not_enforced(e) } else { not_enforced(e) }
}

/// Filesystem: every right Landlock handles is denied except reading the policy's read paths,
/// executing (and reading) its exec paths, and `/dev/null`. On kernels with network rights (ABI
/// 4), TCP binding and connecting are denied too; with scopes (ABI 6), signals to and abstract
/// Unix sockets of processes outside the sandbox.
fn landlock(policy: &Checked) -> Result<()> {
    // The first ABI's rights are refused only where Landlock can't be used at all.
    let ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI::V1))
        .map_err(landlock_not_enforced)?
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(AccessFs::from_all(NEWEST))
        .and_then(|r| r.handle_access(AccessNet::from_all(NEWEST)))
        .and_then(|r| r.scope(Scope::from_all(NEWEST)))
        .map_err(not_enforced)?
        .create()
        .map_err(ruleset_error)?;
    let read = AccessFs::ReadFile | AccessFs::ReadDir;
    let ruleset = ruleset
        .set_compatibility(CompatLevel::HardRequirement)
        .add_rules(path_beneath_rules(&policy.read, read))
        .and_then(|r| r.add_rules(path_beneath_rules(&policy.exec, read | AccessFs::Execute)))
        .and_then(|r| r.add_rules(path_beneath_rules(["/dev/null"], AccessFs::ReadFile | AccessFs::WriteFile)))
        .map_err(not_enforced)?;
    let status = ruleset.restrict_self().map_err(not_enforced)?;
    if !status.no_new_privs {
        return Err(SandboxError::NotEnforced("no_new_privs is not set".into()));
    }
    // The first ABI's rights are a hard requirement, so "partially" means only newer rights are
    // missing on this kernel; "not enforced" means nothing is.
    if status.ruleset == RulesetStatus::NotEnforced {
        return Err(SandboxError::NotEnforced(format!("Landlock is not enforced ({:?})", status.landlock)));
    }
    Ok(())
}

fn arch() -> Option<TargetArch> {
    match std::env::consts::ARCH {
        "x86_64" => Some(TargetArch::x86_64),
        "aarch64" => Some(TargetArch::aarch64),
        _ => None,
    }
}

/// System calls that fail with `EPERM` in the sandbox, unconditionally.
fn denied_calls() -> Vec<libc::c_long> {
    let calls = vec![
        // Network: no socket of any family (a Unix datagram socket can send without
        // `connect`), and no connection or listener on one the program might hold.
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        // io_uring runs operations, connections included, outside the seccomp filter.
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
        // Inspecting or changing other processes.
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_pidfd_getfd,
        libc::SYS_pidfd_open,
        libc::SYS_pidfd_send_signal,
        libc::SYS_kcmp,
        libc::SYS_get_robust_list,
        libc::SYS_migrate_pages,
        libc::SYS_move_pages,
        // System V shared memory, message queues and semaphores: a segment or queue this user can
        // open holds what another process put there, and no path rule covers them.
        libc::SYS_shmget,
        libc::SYS_shmat,
        libc::SYS_shmctl,
        libc::SYS_shmdt,
        libc::SYS_msgget,
        libc::SYS_msgsnd,
        libc::SYS_msgrcv,
        libc::SYS_msgctl,
        libc::SYS_semget,
        libc::SYS_semop,
        libc::SYS_semctl,
        libc::SYS_semtimedop,
        // POSIX message queues, which no path rule covers either: another process of the same user can
        // open one, by name.
        libc::SYS_mq_open,
        libc::SYS_mq_unlink,
        libc::SYS_mq_timedsend,
        libc::SYS_mq_timedreceive,
        libc::SYS_mq_notify,
        libc::SYS_mq_getsetattr,
        // Mounts, namespaces and file handles (ways around path-based rules).
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_chroot,
        libc::SYS_unshare,
        libc::SYS_setns,
        libc::SYS_open_by_handle_at,
        libc::SYS_name_to_handle_at,
        libc::SYS_open_tree,
        libc::SYS_move_mount,
        libc::SYS_fsopen,
        libc::SYS_fsconfig,
        libc::SYS_fsmount,
        libc::SYS_fspick,
        libc::SYS_mount_setattr,
        // The kernel's own extension points and system state.
        libc::SYS_bpf,
        libc::SYS_perf_event_open,
        libc::SYS_userfaultfd,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
        libc::SYS_init_module,
        libc::SYS_finit_module,
        libc::SYS_delete_module,
        libc::SYS_kexec_load,
        libc::SYS_kexec_file_load,
        libc::SYS_swapon,
        libc::SYS_swapoff,
        libc::SYS_reboot,
        libc::SYS_syslog,
    ];
    // x86_64 keeps the older process calls and port I/O; aarch64 has neither.
    #[cfg(target_arch = "x86_64")]
    let calls = [calls, vec![libc::SYS_fork, libc::SYS_vfork, libc::SYS_iopl, libc::SYS_ioperm]].concat();
    calls
}

/// `who` and `which` of `setpriority` and `ioprio_set` for one process.
const PRIO_PROCESS: u64 = 0;
const IOPRIO_WHO_PROCESS: u64 = 1;
// `libc::Ioctl` differs by C library.
#[allow(clippy::unnecessary_cast)]
const TIOCSTI: u64 = libc::TIOCSTI as u64;
#[allow(clippy::unnecessary_cast)]
const TIOCLINUX: u64 = libc::TIOCLINUX as u64;

/// What the main filter refuses with `EPERM`: the calls above, and these when their arguments match.
/// - `clone` without `CLONE_THREAD`: a new process.
/// - A signal to any process but `me`, whose ID `exec` keeps. Landlock's signal scope needs a newer
///   kernel than the rest (ABI 6), and `kill(-1, 9)` must not get through on an older one.
/// - A change to the limits, priority or scheduling of any process but `me`.
/// - `TIOCSTI` and `TIOCLINUX`, which type into a terminal the program inherited, as when
///   `gridcraft-cli sandbox-check` runs a program from a shell.
///
/// Process IDs and `ioctl` requests are compared as the kernel reads them, in 32 bits, so high bits
/// set on the 64-bit argument don't get past a rule.
fn main_rules(me: u32) -> Result<BTreeMap<i64, Vec<SeccompRule>>> {
    let me = u64::from(me);
    let word = |arg: u8, op: SeccompCmpOp, value: u64| SeccompCondition::new(arg, SeccompCmpArgLen::Dword, op, value).map_err(not_enforced);
    let rule = |conditions: Vec<SeccompCondition>| SeccompRule::new(conditions).map_err(not_enforced);
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = denied_calls().into_iter().map(|nr| (nr, Vec::new())).collect();
    let clone_thread = u64::try_from(libc::CLONE_THREAD).map_err(not_enforced)?;
    let new_process = SeccompCondition::new(0, SeccompCmpArgLen::Qword, SeccompCmpOp::MaskedEq(clone_thread), 0).map_err(not_enforced)?;
    rules.insert(libc::SYS_clone, vec![rule(vec![new_process])?]);
    for nr in [libc::SYS_kill, libc::SYS_tkill, libc::SYS_tgkill, libc::SYS_rt_sigqueueinfo, libc::SYS_rt_tgsigqueueinfo] {
        rules.insert(nr, vec![rule(vec![word(0, SeccompCmpOp::Ne, me)?])?]);
    }
    // These take 0 for the caller too.
    for nr in [libc::SYS_prlimit64, libc::SYS_sched_setaffinity, libc::SYS_sched_setscheduler, libc::SYS_sched_setparam, libc::SYS_sched_setattr] {
        rules.insert(nr, vec![rule(vec![word(0, SeccompCmpOp::Ne, me)?, word(0, SeccompCmpOp::Ne, 0)?])?]);
    }
    for (nr, one_process) in [(libc::SYS_setpriority, PRIO_PROCESS), (libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS)] {
        let another = rule(vec![word(1, SeccompCmpOp::Ne, me)?, word(1, SeccompCmpOp::Ne, 0)?])?;
        rules.insert(nr, vec![rule(vec![word(0, SeccompCmpOp::Ne, one_process)?])?, another]);
    }
    rules.insert(libc::SYS_ioctl, vec![rule(vec![word(1, SeccompCmpOp::Eq, TIOCSTI)?])?, rule(vec![word(1, SeccompCmpOp::Eq, TIOCLINUX)?])?]);
    Ok(rules)
}

/// The filters: the first answers `clone3` with `ENOSYS`, so the C library falls back to `clone`,
/// whose flags a filter can read (a thread passes `CLONE_THREAD`; a new process doesn't); the
/// second is [`main_rules`]; on x86_64 a third refuses every x32 call, which carries the same
/// architecture value but other numbers.
fn seccomp() -> Result<()> {
    let arch = arch().ok_or_else(|| SandboxError::Unsupported(format!("no seccomp filter for {}", std::env::consts::ARCH)))?;
    let program = |rules: BTreeMap<i64, Vec<SeccompRule>>, errno: i32| -> Result<BpfProgram> {
        let errno = u32::try_from(errno).map_err(not_enforced)?;
        SeccompFilter::new(rules, SeccompAction::Allow, SeccompAction::Errno(errno), arch).and_then(BpfProgram::try_from).map_err(not_enforced)
    };
    let clone3 = program(BTreeMap::from([(libc::SYS_clone3, Vec::new())]), libc::ENOSYS)?;
    let main = program(main_rules(std::process::id())?, libc::EPERM)?;
    // Landlock already set no_new_privs, which installing a filter without privileges needs.
    if std::env::consts::ARCH == "x86_64" {
        seccompiler::apply_filter(&x32_filter()).map_err(not_enforced)?;
    }
    seccompiler::apply_filter(&clone3).map_err(not_enforced)?;
    seccompiler::apply_filter(&main).map_err(not_enforced)?;
    Ok(())
}

/// The x32 bit of a system call number on x86_64.
const X32_SYSCALL_BIT: u32 = 0x4000_0000;

/// A filter that refuses with `EPERM` every call whose number has the x32 bit. x32 calls report
/// the x86_64 architecture value, so the other filters' architecture check lets them through and
/// their numbers match none of the rules. A call of another architecture is left to the other
/// filters, which end the process.
fn x32_filter() -> BpfProgram {
    // `struct seccomp_data`: the call number at offset 0, the architecture at offset 4.
    const LD_W_ABS: u16 = 0x20;
    const JEQ_K: u16 = 0x15;
    const JGE_K: u16 = 0x35;
    const RET_K: u16 = 0x06;
    const AUDIT_ARCH_X86_64: u32 = 62 | 0x8000_0000 | 0x4000_0000;
    let op = |code: u16, jt: u8, jf: u8, k: u32| seccompiler::sock_filter { code, jt, jf, k };
    let errno = u32::try_from(libc::EPERM).unwrap_or(1);
    vec![
        op(LD_W_ABS, 0, 0, 4),
        op(JEQ_K, 1, 0, AUDIT_ARCH_X86_64),
        op(RET_K, 0, 0, libc::SECCOMP_RET_ALLOW),
        op(LD_W_ABS, 0, 0, 0),
        op(JGE_K, 0, 1, X32_SYSCALL_BIT),
        op(RET_K, 0, 0, libc::SECCOMP_RET_ERRNO | errno),
        op(RET_K, 0, 0, libc::SECCOMP_RET_ALLOW),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filters_build_for_both_architectures() {
        // Building only: installing one would confine the test process.
        for arch in [TargetArch::x86_64, TargetArch::aarch64] {
            let f = SeccompFilter::new(main_rules(4242).unwrap(), SeccompAction::Allow, SeccompAction::Errno(1), arch);
            assert!(f.and_then(BpfProgram::try_from).is_ok(), "{arch:?}");
        }
    }

    #[test]
    fn the_calls_that_reach_another_process_have_rules() {
        let rules = main_rules(4242).unwrap();
        for nr in [
            libc::SYS_kill,
            libc::SYS_tkill,
            libc::SYS_tgkill,
            libc::SYS_rt_sigqueueinfo,
            libc::SYS_rt_tgsigqueueinfo,
            libc::SYS_prlimit64,
            libc::SYS_setpriority,
            libc::SYS_ioprio_set,
            libc::SYS_sched_setaffinity,
            libc::SYS_sched_setscheduler,
            libc::SYS_sched_setparam,
            libc::SYS_sched_setattr,
        ] {
            assert_eq!(rules.get(&nr).map(Vec::len), Some(if nr == libc::SYS_setpriority || nr == libc::SYS_ioprio_set { 2 } else { 1 }), "{nr}");
        }
        for nr in [
            libc::SYS_pidfd_open,
            libc::SYS_pidfd_send_signal,
            libc::SYS_kcmp,
            libc::SYS_get_robust_list,
            libc::SYS_migrate_pages,
            libc::SYS_move_pages,
            libc::SYS_shmget,
            libc::SYS_shmat,
            libc::SYS_msgget,
            libc::SYS_semget,
            libc::SYS_mq_open,
            libc::SYS_mq_unlink,
            libc::SYS_mq_timedsend,
            libc::SYS_mq_timedreceive,
            libc::SYS_mq_notify,
            libc::SYS_mq_getsetattr,
            libc::SYS_syslog,
        ] {
            assert_eq!(rules.get(&nr).map(Vec::len), Some(0), "{nr} is refused outright");
        }
        assert_eq!(rules.get(&libc::SYS_ioctl).map(Vec::len), Some(2));
    }

    #[test]
    fn a_kernel_without_landlock_says_so() {
        // What the tests and a person read to tell this from a sandbox that failed its check.
        assert!(landlock_not_enforced("the crate's text").to_string().contains("Landlock is not enforced: the crate's text"));
    }

    #[test]
    fn only_a_missing_or_disabled_landlock_says_it_is_not_enforced() {
        assert!(says_no_landlock(Some(libc::ENOSYS)));
        assert!(says_no_landlock(Some(libc::EOPNOTSUPP)));
        for errno in [libc::EINVAL, libc::E2BIG, libc::EFAULT, libc::EMFILE, libc::ENOMEM, libc::EPERM] {
            assert!(!says_no_landlock(Some(errno)), "{errno}");
        }
        assert!(!says_no_landlock(None));
        // A ruleset failure of another kind, from the crate itself, on any kernel.
        let e = Ruleset::default().create().err().map(ruleset_error).map(|e| e.to_string());
        assert!(e.is_some_and(|e| !e.contains("Landlock is not enforced")));
    }

    #[test]
    fn the_x32_filter_refuses_numbers_with_the_x32_bit() {
        let f = x32_filter();
        assert_eq!(f.len(), 7);
        // Load the number, then jump past the refusal for numbers below the x32 bit.
        assert_eq!((f[3].code, f[3].k), (0x20, 0));
        assert_eq!((f[4].code, f[4].jt, f[4].jf, f[4].k), (0x35, 0, 1, X32_SYSCALL_BIT));
        assert_eq!(f[5].k, libc::SECCOMP_RET_ERRNO | 1);
    }
}
