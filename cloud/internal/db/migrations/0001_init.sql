-- forge-cloud 初始 schema（15_CLOUD_SERVICE.md §2）。金额一律整数 micros。
-- 尚未上线前如需改表，直接改本文件并重建开发库；上线后一律新增迁移。

-- +goose Up
CREATE TABLE groups (
    id                BIGSERIAL PRIMARY KEY,
    name              TEXT NOT NULL UNIQUE,
    description       TEXT NOT NULL DEFAULT '',
    rate_multiplier   DOUBLE PRECISION NOT NULL DEFAULT 1.0 CHECK (rate_multiplier >= 0),
    concurrency_limit INT NOT NULL DEFAULT 0,
    rpm_limit         INT NOT NULL DEFAULT 0,
    tpm_limit         INT NOT NULL DEFAULT 0,
    allowed_models    TEXT[] NOT NULL DEFAULT '{}',
    is_default        BOOLEAN NOT NULL DEFAULT FALSE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX groups_one_default ON groups (is_default) WHERE is_default;
INSERT INTO groups (name, description, is_default) VALUES ('default', '默认分组', TRUE);

CREATE TABLE users (
    id                   BIGSERIAL PRIMARY KEY,
    email                TEXT NOT NULL UNIQUE,
    password_hash        TEXT NOT NULL,
    nickname             TEXT NOT NULL DEFAULT '',
    role                 TEXT NOT NULL DEFAULT 'user' CHECK (role IN ('user', 'admin')),
    status               TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled')),
    group_id             BIGINT REFERENCES groups (id) ON DELETE SET NULL,
    balance_micros       BIGINT NOT NULL DEFAULT 0,
    concurrency_override INT,
    email_verified       BOOLEAN NOT NULL DEFAULT FALSE,
    avatar_updated_at    TIMESTAMPTZ,
    last_login_at        TIMESTAMPTZ,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE user_avatars (
    user_id      BIGINT PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    content_type TEXT NOT NULL,
    data         BYTEA NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 登录设备 = refresh 会话。token_hash 为当前 refresh token 的 SHA-256；prev_token_hash 用于复用检测。
CREATE TABLE refresh_sessions (
    id              TEXT PRIMARY KEY,
    user_id         BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    device_id       TEXT NOT NULL DEFAULT '',
    device_name     TEXT NOT NULL DEFAULT '',
    platform        TEXT NOT NULL DEFAULT '',
    app_version     TEXT NOT NULL DEFAULT '',
    token_hash      TEXT NOT NULL UNIQUE,
    prev_token_hash TEXT,
    ip              TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    rotated_at      TIMESTAMPTZ,
    expires_at      TIMESTAMPTZ NOT NULL,
    revoked_at      TIMESTAMPTZ
);
CREATE INDEX refresh_sessions_user ON refresh_sessions (user_id);
CREATE INDEX refresh_sessions_prev ON refresh_sessions (prev_token_hash);

-- 平台 API Key：只存 SHA-256；device Key 绑定登录会话。
CREATE TABLE api_keys (
    id                BIGSERIAL PRIMARY KEY,
    user_id           BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name              TEXT NOT NULL DEFAULT '',
    kind              TEXT NOT NULL DEFAULT 'user' CHECK (kind IN ('user', 'device')),
    session_id        TEXT REFERENCES refresh_sessions (id) ON DELETE SET NULL,
    key_hash          TEXT NOT NULL UNIQUE,
    key_prefix        TEXT NOT NULL,
    status            TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked')),
    quota_micros      BIGINT NOT NULL DEFAULT 0,
    used_micros       BIGINT NOT NULL DEFAULT 0,
    concurrency_limit INT NOT NULL DEFAULT 0,
    expires_at        TIMESTAMPTZ,
    last_used_at      TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at        TIMESTAMPTZ
);
CREATE INDEX api_keys_user ON api_keys (user_id);
CREATE INDEX api_keys_session ON api_keys (session_id);

-- 上游账号池。credentials = AES-256-GCM 密文（JSON）；external_id = ChatGPT account id（去重用）。
CREATE TABLE upstream_accounts (
    id                 BIGSERIAL PRIMARY KEY,
    name               TEXT NOT NULL,
    platform           TEXT NOT NULL CHECK (platform IN ('openai', 'anthropic')),
    auth_type          TEXT NOT NULL CHECK (auth_type IN ('oauth', 'apikey')),
    base_url           TEXT NOT NULL DEFAULT '',
    credentials        BYTEA NOT NULL,
    key_hint           TEXT NOT NULL DEFAULT '',
    external_id        TEXT NOT NULL DEFAULT '',
    email              TEXT NOT NULL DEFAULT '',
    plan_type          TEXT NOT NULL DEFAULT '',
    supports_responses BOOLEAN NOT NULL DEFAULT FALSE,
    model_mapping      JSONB NOT NULL DEFAULT '{}'::jsonb,
    proxy_url          TEXT NOT NULL DEFAULT '',
    status             TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled', 'error')),
    priority           INT NOT NULL DEFAULT 50,
    weight             INT NOT NULL DEFAULT 1 CHECK (weight >= 1),
    concurrency_limit  INT NOT NULL DEFAULT 0,
    cooldown_until     TIMESTAMPTZ,
    fail_count         INT NOT NULL DEFAULT 0,
    last_error         TEXT NOT NULL DEFAULT '',
    last_error_at      TIMESTAMPTZ,
    quota              JSONB NOT NULL DEFAULT '{}'::jsonb,
    token_expires_at   TIMESTAMPTZ,
    last_refresh_at    TIMESTAMPTZ,
    last_used_at       TIMESTAMPTZ,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX upstream_accounts_platform ON upstream_accounts (platform, status);
CREATE UNIQUE INDEX upstream_accounts_external ON upstream_accounts (platform, auth_type, external_id) WHERE external_id <> '';

CREATE TABLE account_groups (
    account_id BIGINT NOT NULL REFERENCES upstream_accounts (id) ON DELETE CASCADE,
    group_id   BIGINT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    PRIMARY KEY (account_id, group_id)
);

-- 对外模型目录。价格为每 1M tokens 的 micros。
CREATE TABLE models (
    id                       TEXT PRIMARY KEY,
    display_name             TEXT NOT NULL DEFAULT '',
    platform                 TEXT NOT NULL CHECK (platform IN ('openai', 'anthropic')),
    upstream_model           TEXT NOT NULL DEFAULT '',
    capabilities             JSONB NOT NULL DEFAULT '{}'::jsonb,
    price_input_micros       BIGINT NOT NULL DEFAULT 0,
    price_output_micros      BIGINT NOT NULL DEFAULT 0,
    price_cache_read_micros  BIGINT NOT NULL DEFAULT 0,
    price_cache_write_micros BIGINT NOT NULL DEFAULT 0,
    enabled                  BOOLEAN NOT NULL DEFAULT TRUE,
    is_default               BOOLEAN NOT NULL DEFAULT FALSE,
    sort                     INT NOT NULL DEFAULT 0,
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE plans (
    id                 BIGSERIAL PRIMARY KEY,
    name               TEXT NOT NULL,
    description        TEXT NOT NULL DEFAULT '',
    price_micros       BIGINT NOT NULL DEFAULT 0,
    period_days        INT NOT NULL DEFAULT 30 CHECK (period_days > 0),
    quota_micros       BIGINT NOT NULL DEFAULT 0,
    daily_limit_micros BIGINT NOT NULL DEFAULT 0,
    group_id           BIGINT REFERENCES groups (id) ON DELETE SET NULL,
    enabled            BOOLEAN NOT NULL DEFAULT TRUE,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE subscriptions (
    id                 BIGSERIAL PRIMARY KEY,
    user_id            BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    plan_id            BIGINT NOT NULL REFERENCES plans (id),
    status             TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'expired', 'cancelled')),
    starts_at          TIMESTAMPTZ NOT NULL,
    ends_at            TIMESTAMPTZ NOT NULL,
    quota_micros       BIGINT NOT NULL DEFAULT 0,
    used_micros        BIGINT NOT NULL DEFAULT 0,
    daily_limit_micros BIGINT NOT NULL DEFAULT 0,
    daily_used_micros  BIGINT NOT NULL DEFAULT 0,
    daily_date         DATE,
    group_id           BIGINT REFERENCES groups (id) ON DELETE SET NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX subscriptions_user ON subscriptions (user_id, status, ends_at);

CREATE TABLE redeem_codes (
    id           BIGSERIAL PRIMARY KEY,
    code         TEXT NOT NULL UNIQUE,
    kind         TEXT NOT NULL CHECK (kind IN ('balance', 'plan', 'invite')),
    value_micros BIGINT NOT NULL DEFAULT 0,
    plan_id      BIGINT REFERENCES plans (id) ON DELETE SET NULL,
    batch        TEXT NOT NULL DEFAULT '',
    max_uses     INT NOT NULL DEFAULT 1 CHECK (max_uses >= 1),
    used_count   INT NOT NULL DEFAULT 0,
    status       TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked')),
    expires_at   TIMESTAMPTZ,
    note         TEXT NOT NULL DEFAULT '',
    created_by   BIGINT REFERENCES users (id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX redeem_codes_batch ON redeem_codes (batch);

CREATE TABLE redeem_records (
    id         BIGSERIAL PRIMARY KEY,
    code_id    BIGINT NOT NULL REFERENCES redeem_codes (id) ON DELETE CASCADE,
    user_id    BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (code_id, user_id)
);

-- 余额流水（只追加）。kind: redeem|admin_adjust|usage|signup_bonus|payment|refund
CREATE TABLE balance_ledger (
    id             BIGSERIAL PRIMARY KEY,
    user_id        BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    delta_micros   BIGINT NOT NULL,
    balance_after  BIGINT NOT NULL,
    kind           TEXT NOT NULL,
    ref            TEXT NOT NULL DEFAULT '',
    note           TEXT NOT NULL DEFAULT '',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX balance_ledger_user ON balance_ledger (user_id, created_at DESC);

CREATE TABLE usage_logs (
    id                     BIGSERIAL PRIMARY KEY,
    request_id             TEXT NOT NULL,
    user_id                BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    api_key_id             BIGINT,
    account_id             BIGINT,
    model                  TEXT NOT NULL,
    upstream_model         TEXT NOT NULL DEFAULT '',
    endpoint               TEXT NOT NULL,
    stream                 BOOLEAN NOT NULL DEFAULT FALSE,
    input_tokens           BIGINT NOT NULL DEFAULT 0,
    output_tokens          BIGINT NOT NULL DEFAULT 0,
    cache_read_tokens      BIGINT NOT NULL DEFAULT 0,
    cache_write_tokens     BIGINT NOT NULL DEFAULT 0,
    cost_micros            BIGINT NOT NULL DEFAULT 0,
    charged_balance_micros BIGINT NOT NULL DEFAULT 0,
    charged_plan_micros    BIGINT NOT NULL DEFAULT 0,
    rate_multiplier        DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    status                 TEXT NOT NULL CHECK (status IN ('ok', 'error')),
    error_code             TEXT NOT NULL DEFAULT '',
    http_status            INT NOT NULL DEFAULT 0,
    latency_ms             INT NOT NULL DEFAULT 0,
    first_token_ms         INT NOT NULL DEFAULT 0,
    session_key            TEXT NOT NULL DEFAULT '',
    ip                     TEXT NOT NULL DEFAULT '',
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX usage_logs_user ON usage_logs (user_id, created_at DESC);
CREATE INDEX usage_logs_created ON usage_logs (created_at DESC);
CREATE INDEX usage_logs_account ON usage_logs (account_id, created_at DESC);

-- 在线支付占位（一期无实现）。
CREATE TABLE payment_orders (
    id            BIGSERIAL PRIMARY KEY,
    user_id       BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider      TEXT NOT NULL,
    amount_micros BIGINT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending',
    external_id   TEXT NOT NULL DEFAULT '',
    payload       JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 资料同步：记忆与技能共用全局递增游标。
CREATE SEQUENCE userdata_change_seq;

CREATE TABLE user_settings (
    user_id    BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    namespace  TEXT NOT NULL,
    value      JSONB NOT NULL,
    version    BIGINT NOT NULL DEFAULT 1,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, namespace)
);

CREATE TABLE user_memories (
    user_id    BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    id         TEXT NOT NULL,
    scope      TEXT NOT NULL,
    kind       TEXT NOT NULL,
    content    TEXT NOT NULL DEFAULT '',
    tags       TEXT[] NOT NULL DEFAULT '{}',
    version    BIGINT NOT NULL DEFAULT 1,
    updated_at TIMESTAMPTZ NOT NULL,
    deleted    BOOLEAN NOT NULL DEFAULT FALSE,
    change_seq BIGINT NOT NULL DEFAULT nextval('userdata_change_seq'),
    PRIMARY KEY (user_id, id)
);
CREATE INDEX user_memories_seq ON user_memories (user_id, change_seq);

CREATE TABLE user_skills (
    user_id    BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    files      JSONB,
    sha256     TEXT NOT NULL DEFAULT '',
    size_bytes INT NOT NULL DEFAULT 0,
    version    BIGINT NOT NULL DEFAULT 1,
    updated_at TIMESTAMPTZ NOT NULL,
    deleted    BOOLEAN NOT NULL DEFAULT FALSE,
    change_seq BIGINT NOT NULL DEFAULT nextval('userdata_change_seq'),
    PRIMARY KEY (user_id, name)
);
CREATE INDEX user_skills_seq ON user_skills (user_id, change_seq);

CREATE TABLE system_settings (
    key        TEXT PRIMARY KEY,
    value      JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE audit_logs (
    id         BIGSERIAL PRIMARY KEY,
    actor_id   BIGINT,
    action     TEXT NOT NULL,
    target     TEXT NOT NULL DEFAULT '',
    detail     JSONB NOT NULL DEFAULT '{}'::jsonb,
    ip         TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX audit_logs_created ON audit_logs (created_at DESC);

CREATE TABLE email_codes (
    email      TEXT NOT NULL,
    purpose    TEXT NOT NULL,
    code_hash  TEXT NOT NULL,
    attempts   INT NOT NULL DEFAULT 0,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (email, purpose)
);

-- +goose Down
DROP TABLE IF EXISTS email_codes;
DROP TABLE IF EXISTS audit_logs;
DROP TABLE IF EXISTS system_settings;
DROP TABLE IF EXISTS user_skills;
DROP TABLE IF EXISTS user_memories;
DROP TABLE IF EXISTS user_settings;
DROP SEQUENCE IF EXISTS userdata_change_seq;
DROP TABLE IF EXISTS payment_orders;
DROP TABLE IF EXISTS usage_logs;
DROP TABLE IF EXISTS balance_ledger;
DROP TABLE IF EXISTS redeem_records;
DROP TABLE IF EXISTS redeem_codes;
DROP TABLE IF EXISTS subscriptions;
DROP TABLE IF EXISTS plans;
DROP TABLE IF EXISTS models;
DROP TABLE IF EXISTS account_groups;
DROP TABLE IF EXISTS upstream_accounts;
DROP TABLE IF EXISTS api_keys;
DROP TABLE IF EXISTS refresh_sessions;
DROP TABLE IF EXISTS user_avatars;
DROP TABLE IF EXISTS users;
DROP TABLE IF EXISTS groups;
