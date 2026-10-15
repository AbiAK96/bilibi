-- A named system principal (the scheduler) enqueues without a user. The name
-- must be declared in Authz and is re-proven, with its durable tenant grant, on
-- every attempt. An anonymous 'system' actor is refused at insert; party facets
-- belong to user actors only.
ALTER TABLE base_queue_jobs
    ADD COLUMN system_principal varchar(100) CHECK (system_principal ~ '^[a-z][a-z0-9-]{0,99}$'),
    DROP CONSTRAINT base_queue_jobs_actor_kind_check,
    DROP CONSTRAINT base_queue_jobs_actor_id_check,
    ADD CONSTRAINT base_queue_jobs_actor_check CHECK (
        (actor_kind = 'user' AND actor_id > 0 AND system_principal IS NULL)
        OR (actor_kind = 'system' AND actor_id = 0 AND system_principal IS NOT NULL
            AND person_id IS NULL AND organisation_id IS NULL AND affiliation_id IS NULL));
