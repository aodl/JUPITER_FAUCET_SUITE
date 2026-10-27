//! Evidence for the version-scoped GHSA-qwgh-2vcv-g2f7 exception.
//! Run by security-scan before OSV applies the exception; no canister code.

use sha2::digest::{block_buffer::EagerBuffer, typenum::U4};
use std::panic::{catch_unwind, AssertUnwindSafe};

fn assert_reviewed_lock(lock: &str) {
    let packages: Vec<_> = lock
        .split("[[package]]")
        .filter(|entry| {
            entry
                .lines()
                .any(|line| matches!(line, "name = \"block-buffer\"" | "name = \"block_buffer\""))
        })
        .collect();
    assert_eq!(
        packages.len(),
        1,
        "review every block-buffer version before accepting this advisory"
    );
    let entry = packages[0];
    for expected in [
        "name = \"block-buffer\"",
        "version = \"0.10.4\"",
        "source = \"registry+https://github.com/rust-lang/crates.io-index\"",
        "checksum = \"3078c7629b62d3f0439517fa394996acacc5cbc91c5a20d8c658e77abd503a71\"",
    ] {
        assert!(
            entry.lines().any(|line| line == expected),
            "re-review GHSA-qwgh-2vcv-g2f7: missing {expected}"
        );
    }
}

#[test]
fn exception_requires_exact_reviewed_block_buffer() {
    assert_reviewed_lock(include_str!("../../../Cargo.lock"));
}

#[test]
fn exception_rejects_changed_version_source_or_additional_package() {
    let lock = include_str!("../../../Cargo.lock");
    for changed in [
        lock.replace("version = \"0.10.4\"", "version = \"0.12.0\""),
        lock.replace(
            "source = \"registry+https://github.com/rust-lang/crates.io-index\"",
            "source = \"git+https://example.invalid/unreviewed\"",
        ),
        lock.replace(
            "3078c7629b62d3f0439517fa394996acacc5cbc91c5a20d8c658e77abd503a71",
            "unreviewed checksum",
        ),
        format!("{lock}\n[[package]]\nname = \"block_buffer\"\nversion = \"0.12.0\"\n"),
    ] {
        assert!(catch_unwind(|| assert_reviewed_lock(&changed)).is_err());
    }
}

#[test]
fn caught_compression_panic_preserves_cursor_and_allows_buffer_reuse() {
    // The advisory's EagerBuffer PoC, using the actual locked library through
    // sha2/digest. In 0.10.4, input cannot overwrite the separate position byte.
    let mut buffer = EagerBuffer::<U4>::new(&[1, 2]);
    let failure = catch_unwind(AssertUnwindSafe(|| {
        buffer.digest_blocks(&[3, 0xff], |_| panic!("simulated compression failure"));
    }));
    assert!(failure.is_err());
    assert_eq!(buffer.get_pos(), 2);
    assert_eq!(buffer.get_data(), &[1, 2]);
    let mut compressed = Vec::new();
    buffer.digest_blocks(&[3, 4], |blocks| {
        compressed.extend_from_slice(blocks[0].as_slice());
    });
    assert_eq!(compressed, [1, 2, 3, 4]);
    assert_eq!(buffer.get_pos(), 0);
}

#[test]
fn caught_generator_panic_does_not_corrupt_buffer_position() {
    // 0.10.4 has no ReadBuffer. Its generation API is set_data; a partial
    // generated block is local until its callback returns. Exercise both the
    // partial-block and whole-output-block callback paths, then reuse the buffer.
    for length in [3, 6] {
        let mut buffer = EagerBuffer::<U4>::new(&[1, 2]);
        let mut output = vec![0; length];
        let failure = catch_unwind(AssertUnwindSafe(|| {
            buffer.set_data(&mut output, |blocks| {
                if let Some(block) = blocks.first_mut() {
                    block[0] = 0xff;
                    panic!("simulated generation failure");
                }
            });
        }));
        assert!(failure.is_err());
        assert_eq!(buffer.get_pos(), 2);
        assert_eq!(buffer.get_data(), &[1, 2]);
        buffer.reset();
        buffer.set_data(&mut output, |blocks| {
            for block in blocks {
                block.fill(7);
            }
        });
        assert_eq!(output, vec![7; length]);
        assert_eq!(buffer.get_pos(), length % 4);
    }
}
