-- Migration 14: Enable PostGIS extension
-- The docker-entrypoint-initdb.d script handles fresh databases;
-- this migration covers existing databases that predate the image change.

CREATE EXTENSION IF NOT EXISTS postgis;
