## REMOVED Requirements

### Requirement: Validity

**Reason**: The base capability's validity, over a stack whose link reports scope boundaries.

**Migration**: Compose `uniform-reliable-broadcast` over a broadcast carried by the session link.

### Requirement: Uniform agreement

**Reason**: Already specified by the base capability.

**Migration**: None; the guarantee is unchanged.

### Requirement: An established session prompts a resend

**Reason**: Absorbed into the base capability, where it is conditional on the layer beneath being
able to report an establishment.

**Migration**: The same resend happens when the link supplied reports scope boundaries.

### Requirement: Progress does not depend on the peer returning

**Reason**: Absorbed into the base capability, which states both liveness paths together.

**Migration**: None; the guarantee is unchanged.

### Requirement: No duplication and no creation

**Reason**: Already specified by the base capability.

**Migration**: None; the guarantee is unchanged.
