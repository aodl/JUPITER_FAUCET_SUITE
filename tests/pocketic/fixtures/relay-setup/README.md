# Completed-journal upgrade writer

`historian-recovery-release.wasm.gz` is the frozen canonical recovery-release Historian (SHA-256 `176ad7c4d21b038a8c51db5b5addf19cab3051a337f211f4dbb3675ed2658c64`), whose Historian source is `747613f84122a2062f447ed24fb2764ff4f3106f` and unchanged through the cleanup baseline `33f366382828d4d2939baefb9e90a1ad792731e4`. Its embedded Candid is strictly checked against that baseline. It is a test writer, not a production state snapshot or an approved deployment candidate for this task.

The upgrade regression creates an active setup with its completed journal using this writer, upgrades to the candidate and checks exact journal bytes, active mapping, target/instance tracking and no additional spending. It also checks that reserved memory 31 is untouched. The writer's embedded payload is never used as a candidate factory artifact. The separate current-format paid recovery regression interrupts a supplemental conversion, upgrades and reconciles the same accepted operation before upgrading the completed setup again.

The old pre-recovery writer and its incident-only fixture are retired. Git history retains their provenance.
