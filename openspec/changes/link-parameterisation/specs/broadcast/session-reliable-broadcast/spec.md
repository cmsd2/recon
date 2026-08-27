## REMOVED Requirements

### Requirement: Validity

**Reason**: This is the base capability's validity, over a link whose guarantees lapse at a scope
boundary. It is now that capability composed over such a link rather than a separate protocol.

**Migration**: Compose `reliable-broadcast` over a broadcast carried by the session link.

### Requirement: Agreement is scoped to the sessions carrying the relay

**Reason**: The scoping follows from what the layer beneath can carry, which the base capability now
states as conditional on the link supplied.

**Migration**: None; the guarantee is unchanged when composed over a session-carrying stack, and the
tag it carries is the same.

### Requirement: No duplication and no creation

**Reason**: Already specified by the base capability, over any layer beneath.

**Migration**: None; the guarantee is unchanged.
