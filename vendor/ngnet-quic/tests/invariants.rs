//! Properties the `ngnet-quic` suite *pins* rather than merely exercises.
//!
//! Each is a claim the crate makes about itself that would otherwise decay silently, and
//! each has a test here whose failure is the only warning you would get.
//!
//! Two of them — the sans-I/O scan and the module-layout scan — read the crate's own source
//! textually. That is deliberate. A property like "this crate names no I/O facility" is
//! about the source, not about runtime behaviour, and nothing the compiler checks comes
//! close to expressing it.
//!
//! Because those scans could silently stop matching and turn every claim above them into a
//! claim about the empty set, there are meta-tests at the bottom that prove they still bite.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `.rs` file under a directory, recursively.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(rust_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
    found
}

/// The subtree the asynchronous layer lives in, relative to `src`.
///
/// The core's structural claims are about the code *outside* this directory. Inside it the
/// crate is deliberately asynchronous and deliberately reaches for a socket and a clock, so
/// scanning it for those would flag the feature rather than a defect. The subtree makes its
/// own claims, pinned separately below, and [`the_async_subtree_exists_and_is_scanned`]
/// fails if this path ever stops matching anything — a filter that silently matches nothing
/// turns every test that uses it into a test of nothing.
const ASYNC_SUBTREE: &str = "endpoint";

/// Whether `path` is inside the asynchronous subtree.
fn in_async_subtree(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str() == std::ffi::OsStr::new(ASYNC_SUBTREE))
}

/// The crate's source outside the asynchronous subtree.
fn core_files() -> Vec<PathBuf> {
    rust_files(&crate_root().join("src"))
        .into_iter()
        .filter(|path| !in_async_subtree(path))
        .collect()
}

/// The crate's source inside the asynchronous subtree.
fn async_files() -> Vec<PathBuf> {
    rust_files(&crate_root().join("src"))
        .into_iter()
        .filter(|path| in_async_subtree(path))
        .collect()
}

/// Removes comments and string literals, so prose about a forbidden construct is not
/// mistaken for a use of it.
///
/// This crate documents at length the very things these scans forbid, so without this the
/// scans would fail on the comments explaining why they exist.
fn strip_comments_and_literals(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string = false;

    while let Some(c) = chars.next() {
        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                out.push(c);
            }
            continue;
        }
        if in_block_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block_comment = false;
            }
            continue;
        }
        if in_string {
            if c == '\\' {
                chars.next();
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                in_line_comment = true;
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                in_block_comment = true;
            }
            '"' => in_string = true,
            _ => out.push(c),
        }
    }
    out
}

/// Whether stripped source contains an `unsafe` keyword.
fn mentions_unsafe(code: &str) -> bool {
    code.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|word| word == "unsafe")
}

/// The modules `lib.rs` grants an `unsafe` allowance to, read from `lib.rs` itself.
///
/// Derived rather than listed here, so the two cannot disagree. What is asserted about it is
/// the set's *contents*, below.
fn modules_allowed_unsafe() -> BTreeSet<String> {
    let lib = std::fs::read_to_string(crate_root().join("src").join("lib.rs")).expect("lib.rs");
    let mut allowed = BTreeSet::new();
    let mut granted = false;
    for line in lib.lines() {
        let line = line.trim();
        if line.starts_with("#[allow(unsafe_code)]") {
            granted = true;
            continue;
        }
        if granted {
            if let Some(rest) = line.strip_prefix("mod ") {
                allowed.insert(rest.trim_end_matches(';').trim().to_string());
            }
            // A `#[cfg(...)]` between the allowance and the module is not a module
            // declaration, so keep looking rather than dropping the grant.
            if !line.starts_with("#[") {
                granted = false;
            }
        }
    }
    allowed
}

#[test]
fn unsafe_lives_only_in_the_modules_that_declare_they_need_it() {
    let allowed = modules_allowed_unsafe();
    assert!(
        !allowed.is_empty(),
        "no module was found to be granted `unsafe`, which means the scan stopped matching"
    );

    let mut offenders = Vec::new();
    let mut carriers = BTreeSet::new();

    // Core files only. A subtree file's stem could collide with an allowed core module name
    // -- `endpoint/error.rs` against `error` -- and would then inherit a grant it was never
    // given. `the_async_subtree_contains_no_unsafe_at_all` makes the stronger claim there.
    for path in core_files() {
        let module = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("a file stem")
            .to_string();
        if module == "lib" {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("reading a source file");
        if !mentions_unsafe(&strip_comments_and_literals(&source)) {
            continue;
        }
        carriers.insert(module.clone());
        if !allowed.contains(&module) {
            offenders.push(path);
        }
    }

    assert!(
        offenders.is_empty(),
        "these modules use `unsafe` without being granted it in lib.rs: {offenders:#?}"
    );

    // The other direction: an allowance nothing uses is a boundary wider than the code
    // needs, and the point of the boundary is that it is exactly as wide as it must be.
    let stale: Vec<&String> = allowed.difference(&carriers).collect();
    assert!(
        stale.is_empty(),
        "these modules are granted `unsafe` but do not use it: {stale:?}"
    );
}

#[test]
fn the_allowance_list_is_the_ffi_boundary_and_nothing_else() {
    // The compiler cannot express this: adding an allowance is precisely how the
    // crate-level deny would be silenced. So the expected set is written out, and changing
    // it has to be a deliberate edit here as well as in `lib.rs`.
    let allowed = modules_allowed_unsafe();
    let expected: BTreeSet<String> = [
        "accept",
        "alloc",
        "callbacks",
        "conn",
        "error",
        "ffi",
        "packet",
        "params",
        "path",
        "retain",
        "settings",
        "stream_io",
        // `tls` is deliberately absent, and its absence is the point of this work. The seam
        // itself -- the traits a backend implements -- now contains no `unsafe`, no raw
        // pointer and no foreign type, so it needs no grant. It held one for as long as the
        // seam was two `unsafe` traits handing out an untyped connection handle.
        //
        // The generic translation between ngtcp2's crypto callbacks and the safe TLS seam.
        //
        // This is the module the safe seam exists to create. The `unsafe` a TLS backend
        // used to be required to write -- filling a foreign callback table, holding a
        // connection pointer, promising a handle outlives its connection -- is written here
        // once, generically, instead of once per backend. The list growing by one entry so
        // that every future backend can have none is the trade this work is making, and
        // this is where it is recorded.
        "tls_bridge",
        "tls_ossl",
        // Address validation. Retry tokens and stateless reset tokens are derived by
        // ngtcp2's crypto helpers, and writing a Retry packet needs packet protection --
        // so this cannot live in the asynchronous subtree, which contains no `unsafe` at
        // all. It is a core module for that reason rather than by preference.
        "token",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();

    assert_eq!(
        allowed, expected,
        "the `unsafe` boundary moved; every module here touches the raw bindings, and one \
         that does not should not be on the list"
    );
}

#[test]
fn the_tls_seam_names_nothing_a_backend_cannot_have() {
    // SC-004. The claim is that implementing the seam requires no `unsafe`, no raw pointer and
    // no knowledge of the QUIC library. The first part is enforced structurally -- `tls` has no
    // allowance, so any `unsafe` in it fails to compile -- but the other two are not, because
    // a signature can name a foreign type perfectly safely and still leak the library into an
    // interface that is supposed to hide it.
    //
    // Read textually rather than through the type system, because that is the only way to
    // check "does not mention" as opposed to "does not depend on".
    let source = std::fs::read_to_string(crate_root().join("src/tls.rs")).expect("reading tls.rs");
    // The seam only, not its tests. Those deliberately compare the seam's constants against
    // the raw bindings -- that cross-check is the reason the constants can be restated at all,
    // and it is not part of what a backend author reads.
    let seam = match source.find("#[cfg(test)]") {
        Some(at) => &source[..at],
        None => &source[..],
    };
    let code = strip_comments_and_literals(seam);

    for forbidden in ["*mut ", "*const ", "sys::", "ngtcp2_", "c_void"] {
        assert!(
            !code.contains(forbidden),
            "the TLS seam mentions `{forbidden}`, which a backend author would then have to \
             understand -- the point of the seam is that they do not"
        );
    }
}

#[test]
fn the_decrypt_bridge_copies_nothing() {
    // Phase 7. ngtcp2's core decrypts a received packet into a buffer distinct from the
    // packet, never aliasing the two, so the bridge hands the key its destination and its
    // ciphertext as separate slices and decrypts straight across -- no copy of the
    // ciphertext into the destination first. That is a property of a specific span of code,
    // and it is the kind that decays silently: one `copy_from_slice` added back for the
    // convenience of an in-place primitive would restore exactly the copy this phase removed
    // and never fail a functional test.
    //
    // So the span is named in the source and read textually. Naming it, rather than scanning
    // the whole file, keeps this from tripping over an unrelated copy elsewhere in the
    // bridge -- the sealing path, for one, is allowed its own.
    let source = std::fs::read_to_string(crate_root().join("src/tls_bridge.rs"))
        .expect("reading tls_bridge.rs");
    let start = source
        .find("// region:decrypt-no-copy")
        .expect("the decrypt region is named at its start");
    let end = source
        .find("// endregion:decrypt-no-copy")
        .expect("the decrypt region is named at its end");
    assert!(start < end, "the decrypt region markers are out of order");
    // Strip comments so the prose inside the region -- which talks about copying precisely
    // because the code does not -- cannot be mistaken for the code doing it.
    let region = strip_comments_and_literals(&source[start..end]);

    for forbidden in [
        "copy_from_slice",
        "copy_nonoverlapping",
        "to_vec",
        "extend_from_slice",
    ] {
        assert!(
            !region.contains(forbidden),
            "the decrypt bridge uses `{forbidden}`, which copies the ciphertext it is \
             supposed to decrypt across without touching -- the copy this phase removed"
        );
    }
}

#[test]
fn the_safe_backend_proves_the_seam_needs_no_unsafe() {
    // The strongest available statement, and it is structural rather than asserted: a whole
    // TLS backend lives in a file that *forbids* unsafe code. `forbid` rather than `deny`
    // matters -- a `deny` can be silenced from inside by an allowance, and `forbid` cannot.
    let source = std::fs::read_to_string(crate_root().join("tests/safe_backend.rs"))
        .expect("reading safe_backend.rs");
    assert!(
        source.contains("#![forbid(unsafe_code)]"),
        "the proof backend stopped forbidding unsafe code, which is the only thing that makes \
         it a proof rather than an example"
    );
}

#[test]
fn all_core_module_files_are_flat() {
    // Not style. The scan above derives a module's name from its file stem, so a nested
    // `foo/bar.rs` would be scanned as `bar` -- a name `lib.rs` never declares -- and would
    // be reported as using `unsafe` without a grant, or worse, slip through as `mod`.
    //
    // The asynchronous subtree is exempt because it is a subtree by construction, and
    // because `the_async_subtree_contains_no_unsafe_at_all` below makes the stronger claim
    // that nothing in it uses `unsafe` at all -- which is what the flat rule was protecting.
    let src = crate_root().join("src");
    let nested: Vec<PathBuf> = core_files()
        .into_iter()
        .filter(|p| p.parent() != Some(src.as_path()))
        .collect();

    assert!(
        nested.is_empty(),
        "core module files must sit directly in src/, or the unsafe scan misreads their \
         names: {nested:#?}"
    );
}

/// Facilities a sans-I/O crate must not name.
const FORBIDDEN: &[&str] = &[
    "std::net",
    "std::fs",
    "std::thread",
    "std::time",
    "std::process",
    "std::env",
];

#[test]
fn the_core_reaches_for_no_io_threading_or_time_facility() {
    // The clock is the interesting one. ngtcp2 wants a timestamp on almost every call, and
    // the only way to supply one without reading a clock is to make the caller pass it --
    // which is why `Timestamp` exists at all.
    //
    // `std::net` matters for a subtler reason: socket addresses are unavoidable in a
    // transport library, and the obvious spelling would fail this scan. `core::net` gives
    // the same types with no I/O attached.
    //
    // This is a claim about the core alone. The asynchronous subtree exists precisely to
    // name these things, and has its own, narrower claim below.
    let mut offenders = Vec::new();

    for path in core_files() {
        let source = std::fs::read_to_string(&path).expect("reading a source file");
        let code = strip_comments_and_literals(&source);
        for forbidden in FORBIDDEN {
            if code.contains(forbidden) {
                offenders.push(format!("{} names {forbidden}", path.display()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "the sans-I/O core reached for something it must not: {offenders:#?}"
    );
}

/// Facilities the asynchronous subtree must not reach for either.
///
/// It is allowed a socket and a clock — that is what it is for. It is not allowed to make
/// its own threads or processes, because it takes no executor and spawns nothing: every
/// future it produces is polled by the caller, and a subtree that could spawn would be able
/// to hide work from the caller's runtime.
const FORBIDDEN_IN_SUBTREE: &[&str] = &["std::thread", "std::process", "std::env"];

#[test]
fn the_async_subtree_spawns_nothing_and_runs_nothing() {
    let mut offenders = Vec::new();
    for path in async_files() {
        let source = std::fs::read_to_string(&path).expect("reading a source file");
        let code = strip_comments_and_literals(&source);
        for forbidden in FORBIDDEN_IN_SUBTREE {
            if code.contains(forbidden) {
                offenders.push(format!("{} names {forbidden}", path.display()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "the asynchronous layer acquired a way to run work the caller did not poll: \
         {offenders:#?}"
    );
}

#[test]
fn the_async_subtree_contains_no_unsafe_at_all() {
    // A claim the core cannot make, and the reason the subtree needs no entry in the
    // `unsafe` allowance list in `lib.rs`. Every foreign call lives below this layer; if
    // one appears here, the safe API it is meant to be built on has a hole.
    let mut offenders = Vec::new();
    for path in async_files() {
        let source = std::fs::read_to_string(&path).expect("reading a source file");
        if mentions_unsafe(&strip_comments_and_literals(&source)) {
            offenders.push(path);
        }
    }
    assert!(
        offenders.is_empty(),
        "the asynchronous layer must need no `unsafe`: {offenders:#?}"
    );
}

#[test]
fn the_async_subtree_exists_and_is_scanned() {
    // Without this, deleting or renaming the subtree would turn all three claims above into
    // claims about the empty set, silently and with every test still green.
    let found = async_files();
    assert!(
        !found.is_empty(),
        "no files found under src/{ASYNC_SUBTREE}/; the subtree filter has gone stale and \
         the claims made about the asynchronous layer are now claims about nothing"
    );

    // And the partition must actually partition: a filter matching everything would make
    // the core's claims vacuous instead.
    assert!(
        !core_files().is_empty(),
        "the subtree filter swallowed the whole crate; the core's claims now cover nothing"
    );
}

#[test]
fn no_asynchrony_escapes_the_subtree() {
    // The core is driven, not driving. Its freedom from asynchrony is what lets it be used
    // from blocking code, from any runtime, and from a test with no runtime at all -- so
    // the asynchronous layer is allowed to exist only inside `src/endpoint/`, and nowhere
    // else.
    let mut offenders = Vec::new();
    for path in core_files() {
        let source = std::fs::read_to_string(&path).expect("reading a source file");
        let code = strip_comments_and_literals(&source);
        if code.contains("async fn") || code.contains("async move") || code.contains("await") {
            offenders.push(path);
        }
    }
    assert!(
        offenders.is_empty(),
        "asynchrony must stay inside the `{ASYNC_SUBTREE}` subtree: {offenders:#?}"
    );
}

#[test]
fn an_included_file_cannot_smuggle_code_past_the_scans() {
    // `include_str!` splices a file's contents into what the compiler sees, but not into
    // what the scans above read. A file included from `src/` could therefore carry `unsafe`,
    // an `async fn`, or a forbidden `std::` path that this suite would report as absent.
    //
    // The core makes the strict claim: every file it includes must be inert data, not
    // source. Today that is two PEM certificates.
    //
    // The subtree makes the weaker claim `ngnet-h3` makes -- an include must at least
    // resolve inside the subtree -- because a layer that documents itself with prose needs
    // to include Markdown, and because the subtree's own claims (no `unsafe`, no spawning)
    // are checked over files the scans can see. An include reaching *out* of the subtree
    // would evade the core's scans, which is the hole being closed.
    const INERT: &[&str] = &["pem", "md", "txt", "json"];

    let src = crate_root().join("src");
    let mut offenders = Vec::new();

    for path in rust_files(&src) {
        let source = std::fs::read_to_string(&path).expect("reading a source file");
        // The included path is a string literal, so it survives only in the raw source --
        // which is exactly why this scan reads the unstripped text.
        for capture in source.split("include_str!(").skip(1) {
            let Some(literal) = capture.split('"').nth(1) else {
                continue;
            };
            let extension = literal.rsplit('.').next().unwrap_or("");
            if !INERT.contains(&extension) {
                offenders.push(format!("{} includes {literal}", path.display()));
                continue;
            }
            // And it must actually exist, or the claim is about nothing.
            let target = path
                .parent()
                .expect("a source file has a parent")
                .join(literal);
            assert!(
                target.exists(),
                "{} includes {literal}, which does not exist",
                path.display()
            );

            // An include from inside the subtree must resolve inside it too. Reaching out
            // would let a subtree file pull in text the core's scans never examine.
            if in_async_subtree(&path) {
                let resolved = target.canonicalize().expect("an existing include resolves");
                assert!(
                    in_async_subtree(&resolved),
                    "{} includes {literal}, which resolves outside the subtree",
                    path.display()
                );
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "these includes could carry code the scans cannot see: {offenders:#?}"
    );
}

#[test]
fn the_crate_declares_exactly_one_non_optional_dependency() {
    // Read textually rather than from the built graph, because the claim is about what this
    // crate asks for, not about what happens to be in the workspace lock file.
    let manifest =
        std::fs::read_to_string(crate_root().join("Cargo.toml")).expect("reading Cargo.toml");

    let mut in_dependencies = false;
    let mut declared = Vec::new();
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_dependencies = line == "[dependencies]";
            continue;
        }
        if !in_dependencies || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, rest)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim().split('.').next().expect("a dependency name");
        if rest.contains("optional = true") {
            continue;
        }
        declared.push(name.to_string());
    }

    assert_eq!(
        declared,
        vec!["ngnet-quic-sys".to_string()],
        "the crate's dependency list has changed; its whole shape depends on staying at one"
    );
}

#[test]
fn the_crate_has_no_dev_dependencies() {
    // A test-only dependency is still something a contributor has to build, and it would be
    // the easy way to acquire a certificate generator or an RNG. Those belong in
    // `ngnet-quic-tests`, which is exactly why that crate exists.
    let manifest =
        std::fs::read_to_string(crate_root().join("Cargo.toml")).expect("reading Cargo.toml");
    assert!(
        !manifest.contains("[dev-dependencies]"),
        "the crate has acquired dev-dependencies; test-only needs belong in ngnet-quic-tests"
    );
}

#[test]
fn a_caller_never_needs_unsafe() {
    // The whole point of the crate. Its own tests are the closest thing to a real caller, so
    // if they need `unsafe` to *use* the API, the API is incomplete.
    //
    // Implementing the TLS seam counts as using it. It did not always: the seam was two
    // `unsafe` traits, and `compat_surface` needed an exemption purely to pin their shape.
    // That exemption is gone, and its absence is the clearest single statement of what this
    // work changed -- the file implements a backend, a session and both key kinds, and needs
    // no `unsafe` to do it.
    //
    // The exemptions are named individually rather than by pattern, so that one becoming
    // unnecessary is noticed rather than silently kept.
    let exempt: BTreeSet<&str> = [
        // Calls the raw versioned symbols on purpose: proving they link is the point.
        "versioned_ffi",
        // This file. A scanner for `unsafe` cannot avoid naming it.
        "invariants",
        // Installs a counting `GlobalAlloc`, which is an unsafe trait for reasons that have
        // nothing to do with this crate's API.
        "zero_alloc",
        // Pins `OwnedBytes::from_owner`, which is deliberately `unsafe`: its contract -- that
        // the erased owner lends a slice of stable address and length -- cannot be checked, so
        // it cannot be a safe fn. That is not a hole in the safe API; `OwnedBytes::new` is the
        // safe path and is complete. Pinning that `from_owner` is an `unsafe fn` is the one
        // thing this file must say that it cannot say without writing `unsafe`, exactly as
        // `versioned_ffi` cannot prove the raw symbols link without naming them.
        "compat_surface",
    ]
    .into_iter()
    .collect();

    let mut offenders = Vec::new();
    for path in rust_files(&crate_root().join("tests")) {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if exempt.contains(stem) {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("reading a test file");
        if mentions_unsafe(&strip_comments_and_literals(&source)) {
            offenders.push(path);
        }
    }

    assert!(
        offenders.is_empty(),
        "these tests need `unsafe` to use the crate, which means the safe API has a hole: \
         {offenders:#?}"
    );
}

#[test]
fn every_version_constant_lives_in_one_module() {
    // A wrong struct-version constant is neither a compile error nor a runtime error: it is
    // ngtcp2 misreading the memory behind a pointer. Keeping them in one file is what makes
    // them reviewable, so this pins that they have not spread.
    let mut offenders = Vec::new();
    for path in rust_files(&crate_root().join("src")) {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem == "ffi" {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("reading a source file");
        let code = strip_comments_and_literals(&source);
        // Named individually rather than by suffix: `NGTCP2_ERR_VERSION_NEGOTIATION` and
        // friends also end in `_VERSION` and are ordinary error codes, not struct versions.
        const STRUCT_VERSIONS: &[&str] = &[
            "NGTCP2_PKT_INFO_VERSION",
            "NGTCP2_SETTINGS_VERSION",
            "NGTCP2_CALLBACKS_VERSION",
            "NGTCP2_TRANSPORT_PARAMS_VERSION",
            "NGTCP2_CONN_INFO_VERSION",
        ];
        if STRUCT_VERSIONS.iter().any(|name| code.contains(name)) {
            offenders.push(path);
        }
    }
    assert!(
        offenders.is_empty(),
        "struct-version constants must stay in ffi.rs, where they can be reviewed together: \
         {offenders:#?}"
    );
}

#[test]
fn the_scan_actually_sees_files() {
    // A path filter that stopped matching would make every claim above a claim about an
    // empty set, and would do so silently.
    let files = core_files();
    assert!(
        files.len() >= 15,
        "the scan found only {} core source files, which suggests it stopped matching",
        files.len()
    );
}

#[test]
fn the_subtree_filter_discriminates() {
    // Proves the partition is a partition rather than a predicate that answers the same way
    // for everything -- the failure mode that would make one side's claims vacuous while
    // leaving every test green.
    let src = crate_root().join("src");
    assert!(in_async_subtree(&src.join(ASYNC_SUBTREE).join("mod.rs")));
    assert!(in_async_subtree(
        &src.join(ASYNC_SUBTREE).join("deeper").join("thing.rs")
    ));
    assert!(!in_async_subtree(&src.join("conn.rs")));
    assert!(!in_async_subtree(&src.join("lib.rs")));
}

#[test]
fn the_scanner_would_catch_a_real_violation() {
    let violating = "fn f() { let _ = std::net::SocketAddr::V4; unsafe { g() } }";
    let code = strip_comments_and_literals(violating);
    assert!(code.contains("std::net"));
    assert!(mentions_unsafe(&code));
}

#[test]
fn the_scanner_sees_through_comments_and_literals() {
    // What lets the crate document the very things it forbids.
    let prose = r#"
        //! This module explains why std::net is not used, and why unsafe is confined.
        /* std::thread would also be wrong, and so would unsafe here. */
        fn f() { let _ = "std::fs and unsafe"; }
    "#;
    let code = strip_comments_and_literals(prose);
    assert!(!code.contains("std::net"));
    assert!(!code.contains("std::thread"));
    assert!(!code.contains("std::fs"));
    assert!(!mentions_unsafe(&code));
}

#[test]
fn the_unsafe_word_scanner_does_not_match_substrings() {
    // `unsafely_named_thing` is not an `unsafe` block, and a substring scan would say it
    // was -- which would make the boundary test fail for the wrong reason.
    assert!(!mentions_unsafe("fn unsafely_named() {}"));
    assert!(!mentions_unsafe("let not_unsafe_at_all = 1;"));
    assert!(mentions_unsafe("unsafe { }"));
}
