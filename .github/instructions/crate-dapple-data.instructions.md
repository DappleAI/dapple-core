---
applyTo: "crates/dapple-data/src/**/*.rs"
title: "Dapple Data Crate Guide"
description: "The data access layer of the dapple-prompt workspace."
---

# `dapple-data` Crate Instructions

## Philosophy

### Rust Data Development Philosophy
- Relational Objects should be
- Ensure that queries that return a Relational Object query for the minimal information needed (i.e. key only vs. owned-fields) based on the lifecycle of a given data structure.
- All queries should be built around SQL files via query_file_as!() and be matted to a struct implementing FlatRelationalObject.
- Flat Relational Object SELECT queries should map to an appropriate struct which implements FlatRelationalObject. Relational Object SELECT queries should map to an appropriate struct which implements FlatRelationalObject. It should include the fields to build a Reference Object the Owner Relational Object (if any), all columns of Owned Dependencies and Reference Object columns of Shared Dependents.
- Flat Relational Objects that have dependents must implement the `OrderedMerge` trait. On insert, construct the object's flat result set using `query_file_as!()` together with the crate's `InsertXArgs` and the `INSERT ... RETURNING` pattern. Perform the parent insert first, then invoke insert routines only for the parent's direct dependents — do not insert grandchildren or other non-direct dependents from the parent. Each dependent repeats this pattern recursively until the entire hierarchy is created. After a dependent set is inserted, merge it back into the parent's inserted results using the crate's `merge_` helper (the `merge_`-prefixed merge function for that Relational Object). Preserve deterministic ordering end-to-end by using `UNNEST WITH ORDINALITY` and including the `ord` column in final `SELECT`s so returned rows can be folded and merged reliably by `OrderedMerge`. Important: implement `OrderedMerge` and this insert flow precisely — merging multiple levels at once or inserting non-direct dependents out of place breaks ordering guarantees, reduces performance, and causes serialization/deserialization mismatches. Ensure `InsertXArgs` vectors are equal-length and cover test assertions that `INSERT ... RETURNING` responses fold to the corresponding GET results.


## Crate Overview
The [dapple-data](#crate-dapple-data) crate is responsible for all database interactions in the Dapple Prompt project. It uses SQLx for PostgreSQL database operations with compile-time query verification. This crate provides a clean abstraction layer for data access, ensuring that all database operations are performed safely and efficiently.

The primary focus is on data throughput from the database, integrity and correctness of queries and ordinality of its returned data. This crate is so closely linked to SQL and in turn the database engine and so SQL and database structure are treated in the documentation and code as an integral part of the crate.

## Trait Concepts

### Relational Object
An object that represents exactly a single row stored in a physical table, view,  of a query result set or retrieved via SQLx.

NOTE: This object much exactly represent the query result shape since SQLx doesn't allow for a full Serialization/Deserialization for several reasons.

### Principal Object
A Relational Object that represent data with a lifecycle and is not owned by any other [Principal Object](#principal-object).

### Owned Dependents
A Relational Object can have one or more Owned Dependents who follow the same data lifecycle as the [Principal Object](#principal-object).

### Shared Dependents
that may be associated with multiple principals .

### Associative Dependent
A special instance of Owned Dependent that represent a many-to-many representation between [Principal Object](#principal-object) s.
This is represented by a join table in the database schema.

### Dependent Objects
A catch-all term to refer to any of the dependents mentioned above.

### Versioned Relational Object
Contains a `Version` column set that track the version of changes to the object over time. Each of this type of object
must have a `path_id` column that references the `id` column of the corresponding [Path Relational Object](#path-relational-object) table.
This is always a one-to-on relationship. This relationship should be considered slightly different from the typical
[Principal Object](#principal-object) to Owned Dependent relationship, as the [Versioned Relational Object](#versioned-relational-object) is not strictly owned by the
[Path Relational Object](#path-relational-object), but rather is a historical record of its state over time.

### Path Relational Object
This is a special type of [Principal Object](#principal-object) that represents a specific "path" to versions over time, and is always
associated with on or more [Versioned Relational Objects](#versioned-relational-object). The [Path Relational Object](#path-relational-object) contains a unique `id` column
that is referenced by the `path_id` column of its associated [Versioned Relational Objects](#versioned-relational-object). This relationship is
one-to-many, where one [Path Relational Object](#path-relational-object) can have multiple [Versioned Relational Objects](#versioned-relational-object) associated with it,
each representing a different version of the *Relational Path Object* data over time. Think of a path like a file path in a version control system, where each versioned object represents a snapshot of the file at a specific point in time.

The values here are considered immutable since the path itself does not change, only the versions associated with it.

### Column Set
A group of columns related together via a flattened struct representation. This is mostly used to ensure uniformity
across Relational Objects similar to how a trait works in Rust.

### Flat Relational Object
A flat relational object is a representation of a query resultset row containing a [Relational Object](#relational-object) and all of its
*Owned Dependents* in a single flattened struct, with each column from the [Owned Dependent](#owned-dependent) being appended by
`{table_name}_`. Every *Shared Dependant* will be a flat representation of a  `{table_name}_id` (UUID foreign key). This
is useful for queries that need to retrieve a complete view of an object and its dependents in one go, reducing the
number of database round-trips required. This may not necessarily differ from the [Principal Object](#principal-object), in the cases
where no [Owned Dependent](#owned-dependent) exists but should exist anyway for the sake of easily adding Dependents in the future.

### Object Reference
A lightweight representation of a [Relational Object](#relational-object) that contains its `id` (UUID primary key).

### Data Transfer Object
A representation derived from a Relational Object (the internal model) for external consumption. The `id: Uuid` field is the primary key and IS exposed in DTOs. DTOs are typically `pub`, derive `Serialize` and `JsonSchema`, and map from internal `*Model` types via `From<Model> for DTO` conversions.

* The `id: Uuid` field is the primary key for all non-lookup tables and is safe to expose in DTOs.
* Lookup tables still use `id: i16`/`id: i64` internally and those numeric IDs are NOT exposed in DTOs.
* DTOs must be serializable to both JSON and `schemars::JsonSchema` to ensure they are suitable for API responses and OpenAPI generation.

5. DTOs SHOULD implement the `dapple-data::traits::DataTransferObject` trait which provides a small helper API for consumer-facing types:
    - `id(&self) -> &Uuid` accessor
    - `from_model<M: Into<Self>>(m: M) -> Self` helper (default implementation uses `Into<Self>`)
      DTOs MUST NOT expose numeric lookup-table primary keys.

## Module Organization
### model
`model.rs` contains a submodule for each [Principal Object](#principal-object), which in turn contains a submodule for each
[Owned Dependent](#owned-dependent). Each module contains all structs needed to run and process the results of queries in the `query`
module.

### query
`query.rs` contains a submodule for each [Principal Object](#principal-object), which in turn contains a submodule for each *Dependent Object*, This will provide all of the *Query Functions* which make up the core of the data access layer.
[Owned Dependent](#owned-dependent).

### error


## Technology Stack
SQLx - This crate is built with SQLx for database interactions via queries accessed via the `sqlx::query_as_file!` macros to ensure seperation of Rust and SQL.

### Environment Variables

- `DAPPLE_PROMPT_DATABASE_URL`: PostgreSQL connection string (required)
- `SQLX_OFFLINE`: Set to `true` for offline SQLx query verification

### Database

The project uses SQLx with offline mode enabled. Pre-verified query files are stored in `.sqlx/` directory. When making database queries:

1. Write queries using `query_file_as!` SQLx macro, ensuring compile-time verification of an internally available (pub(crate)) {TableName}Model struct.
2. Run `cargo sqlx prepare` to generate offline query verification files
3. Commit the generated files in `.sqlx/` directory

#### Schema Conventions
##### Overview
The database schema is organized into multiple schemas to separate concerns, improve security and allow for different views of the same data. This workspace will never rely on the schema search path, and all queries explicitly reference the schema they are working with. This will likely change for [SELECT](#select-queries) in the future, but more scaffolding is needed to keep tight control over schema search path based on auth levels.

##### public
exists and is accessible to all services within the dapple ecosystem, it is primarily used by external tools
that complicate using schemas.

##### rev
This schema contains tables relevant to RevOps and their analytical, billing and auditing needs. It will be the largest sized schema by far, since they traditionally need access to this data.

##### web
This schema contains tables relevant to the Dapple Prompt web application and its users. Most of the web's data ends up important to revOps, so this schema will mostly fill up with strictly recent partitioned data of tables from the rev schema.

This may also occassionally contain scratch-pad tables where data only ever pertinent to the web appliation is stored. This should be fairly rare, but is allowed when necessary.

#### auth
This schema contains tables for the nextAuth.js authentication system used by the Dapple Prompt web application. This schema is only accessible by the web application and is not exposed to other services. These tables will never be written to by this workspace outside of integration tests where not relying on the web service helps to isolate what is being tested.

### Table Design
#### Overview
Since this API is focused on data integrity and throughput, the database schema is designed with several key principles in mind:
* Tables must be designed to be append only. UPDATES should never be a part of regular operations and not exposed to regular APIs. These operations will be made available through internal admin APIs with additional logging, auditing and checks in place.
* Soft deletes are required, a separate table should maintain the state of objects as they change over time. For example, a `deleted_at` timestamp may lose data if an oject is "undeleted", so these actions along with any others need to be audited and must adhere to the append-only principle for tables.
* Each [Principal Object](#principal-object) has a dedicated table in the database, with columns representing its attributes.
* *Owned Dependents* have their own tables, with foreign key relationships to their respective [Principal Object](#principal-object) tables.
* *Shared Dependents* also have their own tables, but they are referenced by multiple [Principal Objects](#principal-object) via foreign keys.
* [Versioned Relational Objects](#versioned-relational-object) have a `path_id` column that references the `id` column of the corresponding [Path Relational Object](#path-relational-object) table.
* Indexes are created on frequently queried columns to optimize read performance. Composite indexes are preferred and so indexing should be considered only after careful analysis of query patterns.
* Normalization is applied to reduce data redundancy and is crucial to query performance.
* Foreign key constraints are used to maintain referential integrity between tables. Also, unique key constraints are currently permitted. However, other forms of triggers are generally avoided.
* Timestamps `created_at`, `updated_at`) are included in each table to track record creation and modification times.

#### Table Naming Conventions
- Table names are plural snake_case (e.g., `playbooks`, `users`)
- Tables names for [Associative Dependents](#associative-dependent) are named `{principal_table_singular}_{dependent_table_singular}` (e.g., `playbook_steps`)
- Table names for [Path Relational Objects](#path-relational-object) are named `{principal_table_singular}_paths` (e.g., `playbook_paths`)
- Table Names for that simply add information to another table are named `{principal_table_singular}_{details}` (e.g., `user_details`)

### Column Name Conventions
- Primary key columns are named `id` and are type `UUID` (with `DEFAULT uuidv7()`) for principal/entity tables, or `SMALLINT`/`BIGINT` for lookup/enum tables
- Foreign key columns are named `{related_table_singular}_id` and use `UUID` type (or `SMALLINT`/`BIGINT` when referencing lookup tables)
- Foreign keys are named `fk_{table_name}_id_{related_table_name}_id`
- Timestamps are named `{adjecive}_at` and use `TIMESTAMPTZ` type, all times must be stored with timezone information.
- Version columns are named `version_{component}` (e.g., `version_major`, `version_minor`, `version_patch`) and all three must be present as a[ Column Set](#column-set).

### Queries
#### Overview
A [Query Function](#query-function) is a function that performs a specific database operation, such as retrieving, inserting, updating, or deleting data. Each [Query Function](#query-function) that returns a value is expected to return the [Flat Relational Object](#flat-relational-object) or a `Vec<[Flat Relational Object](#flat-relational-object)>` and is responsible for executing the appropriate SQL query using SQLx's query_file_as! macro.

The corresponding SQL query files are located in the `assets/sql/{principal_query_object}` directory and a subdirectory structure that follows the module structure, organized by [Principal Object](#principal-object) and *Dependent Object*. The [Principal Object](#principal-object) query files are located directly in the `{principal_query_object}` root directory.

### Rust Type Conventions
#### {TableName}
##### Description
Each Data Transfer Object (DTO) contains one public struct and represents the consumer-facing representation derived from the internal [Relational Object](#relational-object) for the table. This
contains only the columns that are safe to expose to consumers of the API and provides appropritate data types for the consumer, particularly by providing.

NOTES:
* this struct should have a field named `id` of type `Uuid` that is the UUID primary key of the table.
* this struct should have a field of Vec<{TableName}> for each [Owned Dependent](#owned-dependent).
* this struct should have a field of `Uuid` for each [Shared Dependent](#shared-dependent), named `{table_singular}_id`.
* If this is a [Versioned Relational Object](#versioned-relational-object), it should have a field named `path_id` of type `Uuid` that represents the primary key of the associated [Path Relational Object](#path-relational-object).

#### {TableName}Model
##### Description
A type representing a [Relational Object](#relational-object) that maps directly to a query result set returning the standard complete object information (meaning including all *Dependent Objects*.

NOTE:
* This struct should have a field named `id` of type `Uuid` that is the UUID primary key of the table.
* this struct should have a field of Vec<{TableName}> for each [Owned Dependent](#owned-dependent).
* this struct should have a field of `Uuid` for each [Shared Dependent](#shared-dependent), named `{table_singular}_id`.
* If this is a [Versioned Relational Object](#versioned-relational-object), it should have a field named `path_id` of type `Uuid` that represents the primary key of the associated [Path Relational Object](#path-relational-object).

#### Inserted{TableName}Model
##### Implements
##### Description
A type representing the data from a table that is part of a [Relational Object](#relational-object) that has can be built off of the RETURNING clause of an INSERT statement. The current version of Postgres is limited to returning only the columns of the table being inserted into, so this struct only contains the columns of the table itself. However, it may not necessarily contain all columns, only those that are needed after an insert operation.

NOTE:
* This struct will often be identical to the {TableName}Model due to [Principal Object](#principal-object) having no *Owned Dependents*. This code duplication is considered a temporary anomaly since [Relational Objects](#relational-object) are expected to gain *Owned Dependents* over time. So, while it will be tempting to optimize this away, it is better to keep the separation for clarity and future-proofing.

##### Implements
##### Visibility
pub(crate)

#### Flat{TableName}Model

##### Description
NOTE:
* This struct will often be identical to the {TableName}Model due to [Principal Object](#principal-object) having no *Owned Dependents*. This code duplication is considered a temporary anomaly since *Relation al Objects* are expected to gain *Owned Dependents* over time. So, while it will be tempting to optimize this away, it is better to keep the separation for clarity and future-proofing.

##### Derives
##### Implements

##### Visibility
pub(crate)

### SQLx Usage Guidelines
- Always use parameterized queries to prevent SQL injection
- Leverage SQLx's compile-time query verification via `query_file_as!` macro, the one exception is tables in auth during integration testing, where the file is inserted directly via `query_as!` macro.
- Use transactions for operations that modify more than one set of Inserts.
- Ensure all database errors return a proper dapple_data::eror::DataError
- Use connection pooling via SQLx's PgPool
- Updates should only be used in very rare cases approved by the data team, prefer Inserts and soft deletes wherever possible. This rule can be broken during iteration testing in cases where we need to intentionally break a record.

## Important Considerations
> If you run into cases where both the Rust code and the query could be incorrect, stop immediately and escalate the issue to the user for clarification.
> Existing Query Functions and Relational Objects, while not flawless, already follow this Philosophy properly and can be used as a guide.
> SQLx serialization/deserialization is based of struct field names and the column errors returned are numbered starting at 0 NOT 1.
> Be careful to ask for and compare against reference git commits to compare how patterns were implemented prior to your changes, this prevents accidentally using changed patterns as reference points.
> Postgres is available for testing, make liberal use of psql and IDE Postgres-related tools to follow data flows through debugging sessions.
> You have access to the pgsql extension tool in VSCode for exploring the database schema and running queries directly against the prompt-db.kube connection, which is the same as $DAPPLE_PROMPT_DATABASE_URL.
