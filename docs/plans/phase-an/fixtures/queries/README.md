# AN.8 motivating-query artifacts

Each SQL file uses only the public, read-only `decomposed_messages` view and
SQLite JSON accessors. The query surface does not encode ATM-specific
workflow types: `category` and the JSON variable keys are discovered through
the public introspection output for the installed template corpus.

`expected-results.json` is hand-calculated from the AN.8 seed corpus. The
native Python-query tests execute these exact committed SQL artifacts through
`atm_query.open_readonly`, so they cover the same SQLite authorizer, statement
boundary, result budget, and Python row conversion used by consumers.

The query path opens only SQLite. It does not read rendered message files,
template files, or mailbox JSON. The older captured Claude inbox parser stays
in the parent fixture directory as the historical file-oriented baseline;
AN.8's query path deliberately replaces that class of file parsing with the
durable decomposed view.

`synthetic-vocabulary.sql` intentionally uses a separate `cycle`/`owner`/
`risk` vocabulary. Its test demonstrates that the same generic view plus JSON
surface answers an analogous span, owner, and assessment rollup without an
ATM-core change.
