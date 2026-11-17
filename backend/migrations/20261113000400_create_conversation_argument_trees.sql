-- On-demand summary trees are separate from the future S-04.4 argument graph.
CREATE TABLE conversation_argument_trees (
    conversation_id UUID PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('pending', 'ready', 'failed')),
    attempt INTEGER NOT NULL CHECK (attempt > 0),
    tree JSONB,
    prompt_version TEXT NOT NULL,
    model TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    CHECK ((status = 'ready') = (tree IS NOT NULL)),
    CHECK (tree IS NULL OR jsonb_typeof(tree) = 'object')
);
