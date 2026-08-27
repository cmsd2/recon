## REMOVED Requirements

### Requirement: Validity within a session

**Reason**: This is best-effort validity over a link whose guarantees lapse at a scope boundary,
which is now the base capability composed over such a link rather than a separate broadcast.

**Migration**: Compose `best-effort-broadcast` over the session link. The guarantee is unchanged.

### Requirement: Session endings and establishments are reported to the layer above

**Reason**: Absorbed into the base capability, where it is conditional on the link beneath
reporting them.

**Migration**: The same reports are emitted when the link supplied reports scope boundaries.

### Requirement: A directed send to one member

**Reason**: Absorbed into the base capability unconditionally, since a directed send is useful over
any link and costs nothing over one without scopes.

**Migration**: Use the directed send on `best-effort-broadcast`.

### Requirement: No duplication and no creation

**Reason**: Already specified by the base capability, over any link.

**Migration**: None; the guarantee is unchanged.

### Requirement: State is bounded by membership

**Reason**: Already specified by the base capability, over any link.

**Migration**: None; the guarantee is unchanged.
