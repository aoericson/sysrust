// Target spec for x86_64-unknown-sysrust
// Place in: compiler/rustc_target/src/spec/targets/x86_64_unknown_sysrust.rs

use crate::spec::{
    Cc, LinkerFlavor, Lld, PanicStrategy, StackProbeType, Target, TargetMetadata, TargetOptions,
};

pub(crate) fn target() -> Target {
    Target {
        llvm_target: "x86_64-unknown-none".into(),
        metadata: TargetMetadata {
            description: Some("sysrust OS (x86_64, no std runtime)".into()),
            tier: Some(3),
            host_tools: Some(false),
            std: Some(true),
        },
        pointer_width: 64,
        arch: "x86_64".into(),
        data_layout: "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128".into(),
        options: TargetOptions {
            os: "sysrust".into(),
            executables: true,
            has_thread_local: false,
            panic_strategy: PanicStrategy::Abort,
            linker_flavor: LinkerFlavor::Gnu(Cc::No, Lld::Yes),
            linker: Some("rust-lld".into()),
            static_position_independent_executables: false,
            stack_probes: StackProbeType::None,
            features: "-mmx,-sse3,-ssse3,-sse4.1,-sse4.2,-avx,-avx2".into(),
            disable_redzone: true,
            // No libc, no dynamic linking
            crt_objects_fallback: None,
            ..Default::default()
        },
    }
}
