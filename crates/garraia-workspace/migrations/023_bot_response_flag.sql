-- Migration 023: add is_bot_response flag to messages
--
-- Marks messages posted by the Garra bot (plan 0240, GAR-759).
-- V1: sender_user_id is still the triggering user's UUID; is_bot_response
-- distinguishes bot from human messages. A future migration will introduce
-- a dedicated bot system user.
--
-- ADD COLUMN with a DEFAULT is a metadata-only change in Postgres 11+ — no
-- table rewrite, no long lock.

ALTER TABLE messages
    ADD COLUMN is_bot_response BOOLEAN NOT NULL DEFAULT FALSE;
