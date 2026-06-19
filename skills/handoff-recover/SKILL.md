---
name: handoff-recover
description: Diagnose capsule integrity, stale claims, target mismatch, and pending approval state when automatic handoff does not arrive.
---

# handoff-recover

Run `handoff:recover` with the current working directory. Report its concrete
`issues`, pending lifecycle state, recovery timestamp, and approval state.

Do not consume, rewrite, or delete a capsule during diagnosis. Expired claim
leases may be recovered automatically to `AVAILABLE`; invalid capsules remain
diagnostic findings and must not be injected. Recommend reinstalling or trusting
hooks only when the package files are valid and the user has reviewed them.
