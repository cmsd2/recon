## REMOVED Requirements

### Requirement: Validity across session endings

**Reason**: The base capability's validity, over a stack whose link reports scope boundaries.

**Migration**: Compose `majority-ack-uniform-reliable-broadcast` over a broadcast carried by the
session link.

### Requirement: Uniform agreement across session endings

**Reason**: Already specified by the base capability; the "across endings" part follows from the
resend the base capability now states.

**Migration**: None; the guarantee is unchanged.

### Requirement: No duplication and no creation

**Reason**: Already specified by the base capability.

**Migration**: None; the guarantee is unchanged.

### Requirement: An established session prompts a resend

**Reason**: Absorbed into the base capability, conditional on the layer beneath reporting an
establishment, including its unconditional and directed character.

**Migration**: None; the behaviour is unchanged when composed over a session-carrying stack.

### Requirement: Resending is the only liveness mechanism, and no peer is ever accused

**Reason**: Absorbed into the base capability, which already needs no detector.

**Migration**: None; the guarantee is unchanged.

### Requirement: Session changes are reported to the layer above

**Reason**: Absorbed into `best-effort-broadcast`, which passes upward whatever the link reports.

**Migration**: The same reports are emitted when the link supplied reports scope boundaries.

### Requirement: The assumption is a correct majority, and its failure blocks rather than diverges

**Reason**: Already specified by the base capability, over any layer beneath.

**Migration**: None; the guarantee is unchanged.
