//! R5 acceptance: the consent gate and the runtime/weights verification, all
//! with synthetic files. No XMem, no PyTorch, no weights are downloaded or read
//! from the network here.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

use frameforge_bridge::{RotoConsent, RotoRuntime, RotoRuntimeError, RotoRuntimeRequest, XMEM_COMMIT, python_candidates};

/// The SHA-256 of the three bytes "abc" (FIPS 180-4 test vector): a real hash of
/// real bytes, so the verification path is exercised end to end.
const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("frameforge-r5-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temporary root");
    root
}

fn touch(path: &Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent directory");
    }
    std::fs::write(path, b"abc").expect("write the fixture");
}

/// A synthetic XMem checkout at a chosen ref layout.
fn fake_checkout(root: &Path, layout: RefLayout) -> PathBuf {
    let checkout = root.join("XMem-official");
    std::fs::create_dir_all(checkout.join("model")).expect("model directory");
    let git = checkout.join(".git");
    std::fs::create_dir_all(&git).expect(".git directory");
    match layout {
        RefLayout::LooseRef => {
            std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").expect("HEAD");
            std::fs::create_dir_all(git.join("refs/heads")).expect("refs directory");
            std::fs::write(git.join("refs/heads/main"), format!("{XMEM_COMMIT}\n")).expect("the ref");
        }
        RefLayout::PackedRef => {
            std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").expect("HEAD");
            std::fs::write(git.join("packed-refs"), format!("# pack-refs with: peeled fully-peeled\n{XMEM_COMMIT} refs/heads/main\n")).expect("packed-refs");
        }
        RefLayout::Detached => {
            std::fs::write(git.join("HEAD"), format!("{XMEM_COMMIT}\n")).expect("detached HEAD");
        }
        RefLayout::OtherCommit => {
            std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").expect("HEAD");
            std::fs::create_dir_all(git.join("refs/heads")).expect("refs directory");
            std::fs::write(git.join("refs/heads/main"), "0000000000000000000000000000000000000000\n").expect("the ref");
        }
    }
    checkout
}

enum RefLayout {
    LooseRef,
    PackedRef,
    Detached,
    OtherCommit,
}

/// A request whose files exist, with the weights' true hash.
fn request(root: &Path, layout: RefLayout) -> RotoRuntimeRequest {
    let python = root.join("python.exe");
    let weights = root.join("XMem.pth");
    let script = root.join("xmem_worker.py");
    touch(&python);
    touch(&weights);
    touch(&script);
    RotoRuntimeRequest { python, xmem_source: fake_checkout(root, layout), weights, worker_script: script, expected_sha256: ABC.to_string() }
}

#[test]
fn a_consented_verified_runtime_resolves_and_describes_itself() {
    let root = temp_root("resolve");
    let runtime =
        RotoRuntime::resolve(&request(&root, RefLayout::LooseRef), &RotoConsent::given("user picked their own XMem.pth")).expect("a verified runtime");

    assert_eq!(runtime.xmem_commit, XMEM_COMMIT);
    assert_eq!(runtime.weights_sha256, ABC);
    assert_eq!(runtime.consent_note, "user picked their own XMem.pth");
    let (program, args) = runtime.worker_command();
    assert!(program.ends_with("python.exe"), "{program:?}");
    assert_eq!(args.first().map(String::as_str), Some("-u"), "the worker's reports must not be buffered");
    assert!(runtime.describe().contains(&XMEM_COMMIT[..7]), "{}", runtime.describe());
}

#[test]
fn heads_that_are_loose_packed_or_detached_all_read_the_same_commit() {
    for layout in [RefLayout::LooseRef, RefLayout::PackedRef, RefLayout::Detached] {
        let root = temp_root("refs");
        let runtime = RotoRuntime::resolve(&request(&root, layout), &RotoConsent::given("test")).expect("the ref layout should resolve");
        assert_eq!(runtime.xmem_commit, XMEM_COMMIT);
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[test]
fn consent_comes_before_anything_is_read() {
    let root = temp_root("consent");
    // Paths that do not exist: if the gate let this through, the error would be
    // "missing", not "not consented".
    let request = RotoRuntimeRequest {
        python: root.join("nowhere").join("python.exe"),
        xmem_source: root.join("nowhere"),
        weights: root.join("nowhere").join("XMem.pth"),
        worker_script: root.join("nowhere").join("xmem_worker.py"),
        expected_sha256: ABC.to_string(),
    };
    let refused = RotoRuntime::resolve(&request, &RotoConsent::refused("not now")).expect_err("no consent, no runtime");
    assert!(matches!(refused, RotoRuntimeError::NotConsented(ref note) if note.as_str() == "not now"), "{refused}");
    assert!(refused.to_string().contains("not consented"), "{refused}");
}

#[test]
fn weights_that_are_not_the_promised_bytes_are_refused() {
    let root = temp_root("hash");
    let mut request = request(&root, RefLayout::LooseRef);
    request.expected_sha256 = "1111111111111111111111111111111111111111111111111111111111111111".to_string();
    let error = RotoRuntime::resolve(&request, &RotoConsent::given("test")).expect_err("a hash mismatch is refused");
    match error {
        RotoRuntimeError::WeightsHash { expected, found } => {
            assert_eq!(expected, "1111111111111111111111111111111111111111111111111111111111111111");
            assert_eq!(found, ABC, "the report names the hash actually found");
        }
        other => panic!("expected a hash mismatch, got {other}"),
    }
}

#[test]
fn a_checkout_at_another_commit_is_refused_with_both_ids() {
    let root = temp_root("commit");
    let error = RotoRuntime::resolve(&request(&root, RefLayout::OtherCommit), &RotoConsent::given("test")).expect_err("the wrong commit is refused");
    match error {
        RotoRuntimeError::Commit { expected, found } => {
            assert_eq!(expected, XMEM_COMMIT);
            assert_eq!(found, "0000000000000000000000000000000000000000");
        }
        other => panic!("expected a commit mismatch, got {other}"),
    }
}

#[test]
fn a_directory_without_the_model_folder_is_not_an_xmem_checkout() {
    let root = temp_root("not-checkout");
    let mut request = request(&root, RefLayout::LooseRef);
    std::fs::remove_dir_all(request.xmem_source.join("model")).expect("drop the model directory");
    let error = RotoRuntime::resolve(&request, &RotoConsent::given("test")).expect_err("a random git directory is not XMem");
    assert!(matches!(error, RotoRuntimeError::NotACheckout { .. }), "{error}");
}

#[test]
fn a_bad_expected_hash_is_reported_before_the_weights_are_hashed() {
    let root = temp_root("bad-hash");
    let mut request = request(&root, RefLayout::LooseRef);
    request.expected_sha256 = "abc".to_string();
    let error = RotoRuntime::resolve(&request, &RotoConsent::given("test")).expect_err("a short hash is not a hash");
    assert!(matches!(error, RotoRuntimeError::BadHash(_)), "{error}");
}

#[test]
fn the_consented_note_reaches_the_status_line() {
    let root = temp_root("note");
    let runtime = RotoRuntime::resolve(&request(&root, RefLayout::LooseRef), &RotoConsent::given("weights checked by hand")).expect("resolved");
    assert!(runtime.describe().contains("weights checked by hand"), "{}", runtime.describe());
    assert!(runtime.describe().contains("ba7816bf8f01"), "the short hash is shown: {}", runtime.describe());
}

#[test]
fn python_candidates_prefers_the_configured_interpreter_and_never_runs_anything() {
    let root = temp_root("python");
    let configured = root.join("custom-python.exe");
    touch(&configured);
    let first = root.join("first");
    let second = root.join("second");
    touch(&first.join("python.exe"));
    touch(&second.join("py.exe"));

    let candidates = python_candidates(Some(&configured), &[first, second]);
    assert_eq!(candidates.first(), Some(&configured), "the configured interpreter is first");
    assert_eq!(candidates.len(), 3, "then the PATH names, once each: {candidates:?}");
    assert!(candidates.iter().any(|path| path.ends_with("py.exe")));

    // A configured path that does not exist is skipped, not trusted.
    let missing = root.join("gone.exe");
    let candidates = python_candidates(Some(&missing), &[root.join("first")]);
    assert_eq!(candidates, vec![root.join("first").join("python.exe")]);
}
