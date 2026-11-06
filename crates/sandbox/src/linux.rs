//! Linux: the launcher confines itself with Landlock, then seccomp, then `exec`s the program,
//! which inherits both.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use landlock::{
    ABI, Access, AccessFs, AccessNet, CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope, path_beneath_rules,
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

pub fn confine(policy: &Checked, _: &[OsString]) -> Result<()> {
    landlock(policy)?;
    seccomp()
}

pub fn exec(program: &Path, args: &[OsString]) -> Result<std::convert::Infallible> {
    crate::exec_unix(program, args)
}

fn not_enforced(e: impl std::fmt::Display) -> SandboxError {
    SandboxError::NotEnforced(e.to_string())
}

/// A kernel without Landlock fails while the rules are made, with the Landlock crate's own text.
fn landlock_not_enforced(e: impl std::fmt::Display) -> SandboxError {
    SandboxError::NotEnforced(format!("Landlock is not enforced: {e}"))
}

/// Filesystem: every right Landlock handles is denied except reading the policy's read paths,
/// executing (and reading) its exec paths, and `/dev/null`. On kernels with network rights (ABI
/// 4), TCP binding and connecting are denied too; with scopes (ABI 6), signals to and abstract
/// Unix sockets of processes outside the sandbox.
fn landlock(policy: &Checked) -> Result<()> {
    let ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI::V1))
        .and_then(|r| r.set_compatibility(CompatLevel::BestEffort).handle_access(AccessFs::from_all(NEWEST)))
        .and_then(|r| r.handle_access(AccessNet::from_all(NEWEST)))
        .and_then(|r| r.scope(Scope::from_all(NEWEST)))
        .and_then(|r| r.create())
        .map_err(landlock_not_enforced)?;
    let read = AccessFs::ReadFile | AccessFs::ReadDir;
    let ruleset = ruleset
        .set_compatibility(CompatLevel::HardRequirement)
        .add_rules(path_beneath_rules(&policy.read, read))
        .and_then(|r| r.add_rules(path_beneath_rules(&policy.exec, read | AccessFs::Execute)))
        .and_then(|r| r.add_rules(path_beneath_rules(["/dev/null"], AccessFs::ReadFile | AccessFs::WriteFile)))
        .map_err(landlock_not_enforced)?;
    let status = ruleset.restrict_self().map_err(landlock_not_enforced)?;
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
    ];
    // x86_64 keeps the older process calls and port I/O; aarch64 has neither.
    #[cfg(target_arch = "x86_64")]
    let calls = [calls, vec![libc::SYS_fork, libc::SYS_vfork, libc::SYS_iopl, libc::SYS_ioperm]].concat();
    calls
}

/// The filters: the first answers `clone3` with `ENOSYS`, so the C library falls back to `clone`,
/// whose flags a filter can read (a thread passes `CLONE_THREAD`; a new process doesn't); the
/// second refuses with `EPERM` the calls above and `clone` without `CLONE_THREAD`; on x86_64 a
/// third refuses every x32 call, which carries the same architecture value but other numbers.
fn seccomp() -> Result<()> {
    let arch = arch().ok_or_else(|| SandboxError::Unsupported(format!("no seccomp filter for {}", std::env::consts::ARCH)))?;
    let program = |rules: BTreeMap<i64, Vec<SeccompRule>>, errno: i32| -> Result<BpfProgram> {
        let errno = u32::try_from(errno).map_err(not_enforced)?;
        SeccompFilter::new(rules, SeccompAction::Allow, SeccompAction::Errno(errno), arch).and_then(BpfProgram::try_from).map_err(not_enforced)
    };
    let clone3 = program(BTreeMap::from([(libc::SYS_clone3, Vec::new())]), libc::ENOSYS)?;
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = denied_calls().into_iter().map(|nr| (nr, Vec::new())).collect();
    let flag = |arg: u8, len: SeccompCmpArgLen, op: SeccompCmpOp, value: u64| {
        SeccompCondition::new(arg, len, op, value).and_then(|c| SeccompRule::new(vec![c])).map_err(not_enforced)
    };
    let clone_thread = u64::try_from(libc::CLONE_THREAD).map_err(not_enforced)?;
    rules.insert(libc::SYS_clone, vec![flag(0, SeccompCmpArgLen::Qword, SeccompCmpOp::MaskedEq(clone_thread), 0)?]);
    let main = program(rules, libc::EPERM)?;
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
            let rules: BTreeMap<i64, Vec<SeccompRule>> = denied_calls().into_iter().map(|nr| (nr, Vec::new())).collect();
            let f = SeccompFilter::new(rules, SeccompAction::Allow, SeccompAction::Errno(1), arch);
            assert!(f.and_then(BpfProgram::try_from).is_ok(), "{arch:?}");
        }
    }

    #[test]
    fn a_kernel_without_landlock_says_so() {
        // What the tests and a person read to tell this from a sandbox that failed its check.
        assert!(landlock_not_enforced("the crate's text").to_string().contains("Landlock is not enforced: the crate's text"));
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
