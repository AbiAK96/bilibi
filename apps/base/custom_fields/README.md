# Base Custom Fields

`base/custom_fields` owns tenant-specific definitions for record types that opt in through `RecordType`. The record owner continues to store values in its own JSONB column and validates every submitted patch inside its write transaction.

The public facade is `base/custom_fields.fields`. It lists definitions and supports define, update, archive, restore and owner-side validation. Definition writes use `MutationBoundary`, require `admin.custom.fields.manage` and the opted-in record type's capability, and enforce immutable keys and types.

Choice values are stable codes; an option with `active: false` is retired and remains valid only when preserving an existing value. Reference fields name a Grid table. Reference readability validation is not complete yet; callers must not treat a syntactically valid reference as authorized until the Grid runtime lookup contract lands.

The owner opt-in and Grid/Authz runtime provider contracts are described in [ADR 07](../../../docs/architecture/decisions/07-tenant-defined-custom-fields.md). Core Company is the first adopter. Reporting and shared exports are out of scope.

Limits currently enforced in the facade are implementation defaults. Operator settings, the administration page, Grid runtime projection/filter/sort, Authz's dynamic restriction catalog, and agent definition operations remain to be completed before this module is production-ready.
