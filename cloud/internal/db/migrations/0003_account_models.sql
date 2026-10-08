-- 每个渠道的模型范围；空数组保留既有账号不限模型的行为。
-- +goose Up
ALTER TABLE upstream_accounts
    ADD COLUMN allowed_models JSONB NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(allowed_models) = 'array');

-- +goose Down
ALTER TABLE upstream_accounts DROP COLUMN allowed_models;
