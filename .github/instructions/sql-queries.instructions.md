---
applyTo: "crates/dapple-data/assets/sql/**/*.sql"
title: "SQL Query Instructions"
description: "Guidelines for writing SQL queries for the dapple-data crate."
---

# `dapple-data` SQL Query Instructions

## Philosophy

### General Philosophy
- The relational database server's strong point is not a proxy server and so trips to the database should be minimized.
- Data should be written to Postgres tables append-only fashion wherever possible to avoid mutex contention and frequent updates to the middle of file storage.
- Relational Objects are defined by their lifecycle and only own Dependent Relational Objects that exist for exactly the lifetime of the Principal Object.
  In theory, any Data Transfer Object (DTO) could be cached forever, assuming the code never needed to change its shape in future versions.
- Queries, including INSERTs, should almost always be set-wise and take column-wise array input parameters wherever possible to maximize efficiency. The interface should be similar to those for building series for dataframes in `polars`.

### SQL Development Philosophy
- Queries almost always take in column-wise array input parameters must use `UNNEST WITH ORDANILITY` and provide an ord column containing the ordinality function to expand them into rows for set-wise processing. This allows queries to shine inserting a parent, getting back the ids and passing it along as params to the queries creating the Dependent Relational Objects.
- In queries the final returned rows should be ordered by ord (if it exists) followed by DESC ordering of Principal Object ids then moving through Dependent Relational Objects ids in the appropriate hierarchical order.
- Outside specialized search queries, a Relational Object should only need an INSERT, SELECT by ids and a PATCH query.
- Path Objects do not have their own queries for INSERT statements and are instead inserted in the path CTE of the Versioned Object INSERT and should be left unchanged on PATCH.
- Relational Object SELECT queries should map to an appropriate struct which implements FlatRelationalObject. It should include the fields to build a Reference Object the Owner Relational Object (if any), all columns of Owned Dependencies and Reference Object columns of Shared Dependents.

# SQL Query Instructions

## SELECT Queries
### Overview
These functions are named `select_{object_name}_by_{column_name}` and are responsible for retrieving data from the database based on specific criteria. They return either a single [Flat Relational Object](#flat-relational-object) or a `Vec<[Flat Relational Object](#flat-relational-object)>` depending on whether the query is expected to return one or multiple rows.

### args CTE


## INSERT Queries
### Overview
These functions are named `insert_{object_name}_model` and are responsible for inserting new data into the database. They take in the necessary column-wise set parameters (or occasionally a single parameter) to create a new record and return the inserted *Inserted Relational Object*. Note that RETURNING clauses in Postgres only return columns from the table being inserted into, so the returned object may not contain all columns from *Owned Dependents* and will need to be JOINED in with other data to match the contract set forth in [Flat Relational Object](#flat-relational-object).

For object dependencies, we cannot include them in the same insert, because we cannot sanely pass along an array of composite types as parameters to a SQL query in Postgres. Therefore, we need to perform separate insert operations for each [Owned Dependent](#owned-dependent) after the main [Principal Object](#principal-object) insert is complete. In this case the inserted objects `id` values are passed along with the array of values passed along to the query function.

### Returned Set
The query should return all the rows that were inserted, which is almost always multiple inserted rows since this API always works in set-wise operations. It will make use of several of the CTEs listed below.  The returned set should be ordered by the ordinality of the input args to ensure that the returned rows match the order of the input data. INSERTS and SELECTS are often chained with full sets of data being passed along from query function to query function.

### args CTE
#### Overview
This CTE defines the input parameters for the insert operation. Each parameter contains a column-wise array for some time passed along to the other CTEs or the main INSERT statement.

The parameters here are called together in a single `UNNEST (...) WITH ORDINATILY AS o (...)` clause with the last column given the name `ord` which will hold the oridinality of the args passed along.

This value is mandatory for returning things in the correct order when other tables are joined into the final result to meet the contract of [Flat Relational Object](#flat-relational-object).

#### Returned Set
The args as a set of rows with each column being one of the input parameters and an additional `ord` column representing the ordinality of the input data.

### paths CTE
#### Overview
This CTE is only present in the case of inserting into a [Versioned Relational Object](#versioned-relational-object). It is responsible for inserting a new [Path Relational Object](#path-relational-object) on the column values read from *args CTE* or *input CTE*. This CTE will perform an INSERT operation to attempt to insert the [Path Relational Object](#path-relational-object) exists before proceeding with the main INSERT operation. If the path already exists or there are any key conflicts, the INSERT would fail, since this should be handle as a PATCH operation.

#### Returned Set
The args as a set of rows with each column being one of the input parameters and an additional `ord` column representing the ordinality of the input data, along with the `path_id` column representing the primary key of the [Path Relational Object](#path-relational-object).

### inputs CTE
#### Overview
This CTE is optional but often needed in order to prepare data for the main INSERT operation, such as transforming data types or performing lookups on *Shared Dependents* to retrieve their UUID primary keys based on the `id` (UUID) passed in via the args CTE. Adding JOINs to the objs INSERT...SELECT statement could lead to duplication of rows or other unintended consequences, so this CTE is used to prepare the data in a clean way.

A second common and important use case is to join in *Shared Dependents* to ensure that the passed in `id` (UUID) values are valid and exist in the database before proceeding with the main INSERT operation. This also provides a place to do a GROUP_ARRAY of the ids of the *Shared Dependents* to be used in the main INSERT operation.

### objs CTE
#### Overview
This CTE performs the main INSERT operation into the table representing the [Principal Object](#principal-object). It takes the form of an `INSERT...SELECT` joining in data from the `args` or `inputs` CTEs and any other preparatory CTEs (such as paths) to perform the INSERT operation.

#### Returned Set
Due to Postgres limitations, the RETURNING clause of an INSERT statement can only return columns from the table being inserted into. Therefore, this CTE returns only the columns of the inserted table that are needed and will need to joined further.

### PATCH Queries
#### Overview
These queries are techinically just INSERT statements that serve in place of an `UPDATE` for [Versioned Relational Objects](#versioned-relational-object). Since these objects are immutable once created, any changes to them require the creation of a new version. Therefore, the `patch_{object_name}` query function serves as a way to add an updated *version* of the [Versioned Relational Object](#versioned-relational-object).

The structure is very similar to the `INSERT`, query with the addition of incrementing the version number based on the existing highest version for the given `path_id`.

### args CTE
#### Overview
This CTE defines the input parameters for the insert operation. This will not contain the path information as in the case of the INSERT and will instead only provide the `id` (UUID). Each parameter contains a column-wise array for some time passed along to the other CTEs or the main INSERT statement.

The parameters here are called together in a single `UNNEST (...) WITH ORDINATILY AS o (...)` clause with the last column given the name `ord` which will hold the originality of the args passed along.

This value is mandatory for returning things in the correct order when other tables are joined into the final result to meet the contract of [Flat Relational Object](#flat-relational-object).

#### Returned Set
The args as a set of rows with each column being one of the input parameters and an additional `ord` column representing the ordinality of the input data.

### paths CTE
#### Overview
This CTE will always be present.  It is responsible for retrieving the `path_id` of the associated [Path Relational Object](#path-relational-object) based on the `path_id` (UUID) passed in via the args CTE. This will look up [Path Relational Object](#path-relational-object) and pull some additional needed information like the latest version of the [Versioned Relational Object](#versioned-relational-object).

#### Returned Set
The args as a set of rows with each column being one of the input parameters and an additional `ord` column representing the ordinality of the input data, along with the `path_id` column representing the primary key of the [Path Relational Object](#path-relational-object).

### inputs CTE
#### Overview
This CTE is optional but often needed in order to prepare data for the main INSERT operation, such as transforming data types or performing lookups on *Shared Dependents* to retrieve their UUID primary keys based on the `id` (UUID) passed in via the args CTE. Adding JOINs to the objs INSERT...SELECT statement could lead to duplication of rows or other unintended consequences, so this CTE is used to prepare the data in a clean way.

A second common and important use case is to join in *Shared Dependents* to ensure that the passed in `id` (UUID) values are valid and exist in the database before proceeding with the main INSERT operation. This also provides a place to do a GROUP_ARRAY of the ids of the *Shared Dependents* to be used in the main INSERT operation.

### objs CTE
#### Overview
#### Returned Set
This CTE performs the main INSERT operation into the table representing the [Principal Object](#principal-object). It uses data from the `args` or `inputs` CTE and any other preparatory CTEs (such as paths) to perform the INSERT operation.

### DELETE Queries
#### Overview
For now just don't do DELETEs.

## Important Considerations
> If you run into cases where both the Rust code and the query could be incorrect, stop immediately and escalate the issue to the user for clarification.
> Existing Query Functions and Relational Objects, while not flawless, already follow this Philosophy properly and can be used as a guide.
> SQLx serialization/deserialization is based of struct field names and the column errors returned are numbered starting at 0 NOT 1.
> Be careful to ask for and compare against reference git commits to compare how patterns were implemented prior to your changes, this prevents accidentally using changed patterns as reference points.
> Postgres is available for testing, make liberal use of psql and IDE Postgres-related tools to follow data flows through debugging sessions.
> You have access to the pgsql extension tool in VSCode for exploring the database schema and running queries directly against the prompt-db.kube connection, which is the same as $DAPPLE_PROMPT_DATABASE_URL.
