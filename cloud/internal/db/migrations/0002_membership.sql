-- 会员梯度与额度计费（15_CLOUD_SERVICE.md §11，D-042）。参照 Cursor 个人方案：
-- Hobby / Pro / Pro+ / Ultra，按月或按年（年付 8 折）；两个用量池（api = 第三方模型按 API 价，
-- forge = 平台模型）；档位订阅的套餐内额度按月重置；额度用完后按量付费（扣余额），可设每周期上限。

-- +goose Up
ALTER TABLE plans
    ADD COLUMN tier                TEXT    NOT NULL DEFAULT '',
    ADD COLUMN tier_rank           INT     NOT NULL DEFAULT 0,
    ADD COLUMN tagline             TEXT    NOT NULL DEFAULT '',
    ADD COLUMN features            TEXT[]  NOT NULL DEFAULT '{}',
    ADD COLUMN price_yearly_micros BIGINT  NOT NULL DEFAULT 0 CHECK (price_yearly_micros >= 0),
    ADD COLUMN forge_quota_micros  BIGINT  NOT NULL DEFAULT 0 CHECK (forge_quota_micros >= 0),
    ADD COLUMN highlight           BOOLEAN NOT NULL DEFAULT FALSE;
CREATE UNIQUE INDEX plans_tier ON plans (tier) WHERE tier <> '';

ALTER TABLE models
    ADD COLUMN pool TEXT NOT NULL DEFAULT 'api' CHECK (pool IN ('api', 'forge'));

-- used_micros / forge_used_micros 属于 cycle_start 所在的用量周期（NULL = starts_at）；周期滚动时惰性归零。
ALTER TABLE subscriptions
    ADD COLUMN forge_quota_micros BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN forge_used_micros  BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN usage_cycle        TEXT   NOT NULL DEFAULT 'period' CHECK (usage_cycle IN ('period', 'month')),
    ADD COLUMN cycle_start        TIMESTAMPTZ,
    ADD COLUMN billing_interval   TEXT   NOT NULL DEFAULT '' CHECK (billing_interval IN ('', 'month', 'year')),
    ADD COLUMN value_micros       BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN source             TEXT   NOT NULL DEFAULT 'grant' CHECK (source IN ('grant', 'redeem', 'purchase')),
    ADD COLUMN order_id           BIGINT;

-- on_demand_enabled 默认开：保持「套餐额度不足扣余额」的旧行为。free_* 是 Hobby 免费额度（UTC 自然月）。
ALTER TABLE users
    ADD COLUMN on_demand_enabled      BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN on_demand_limit_micros BIGINT  NOT NULL DEFAULT 0 CHECK (on_demand_limit_micros >= 0),
    ADD COLUMN free_cycle_start       DATE,
    ADD COLUMN free_api_used_micros   BIGINT  NOT NULL DEFAULT 0,
    ADD COLUMN free_forge_used_micros BIGINT  NOT NULL DEFAULT 0;

ALTER TABLE usage_logs
    ADD COLUMN pool TEXT NOT NULL DEFAULT 'api';
CREATE INDEX usage_logs_user_on_demand ON usage_logs (user_id, created_at) WHERE charged_balance_micros > 0;

ALTER TABLE payment_orders
    ADD COLUMN kind                     TEXT   NOT NULL DEFAULT 'topup' CHECK (kind IN ('topup', 'subscription')),
    ADD COLUMN plan_id                  BIGINT REFERENCES plans (id) ON DELETE SET NULL,
    ADD COLUMN billing_interval         TEXT   NOT NULL DEFAULT '',
    ADD COLUMN mode                     TEXT   NOT NULL DEFAULT '',
    ADD COLUMN list_price_micros        BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN credit_micros            BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN replaces_subscription_id BIGINT,
    ADD COLUMN subscription_id          BIGINT,
    ADD COLUMN pay_url                  TEXT   NOT NULL DEFAULT '',
    ADD COLUMN note                     TEXT   NOT NULL DEFAULT '',
    ADD COLUMN paid_at                  TIMESTAMPTZ;
CREATE INDEX payment_orders_user ON payment_orders (user_id, created_at DESC);

-- 内置档位（不绑定分组：订阅用户沿用自己的分组选号，避免新分组里没有上游账号）。
-- 价格与 Cursor 一致；api 池额度同 Cursor（$20 / $70 / $400）；forge 池 Cursor 未公布，
-- 这里按 Pro 的 3 倍 / 20 倍给默认值，上线前由运营者在管理后台调整。
INSERT INTO plans (name, description, tier, tier_rank, tagline, features, price_micros, price_yearly_micros,
                   period_days, quota_micros, forge_quota_micros, highlight, enabled)
SELECT v.name, v.description, v.tier, v.tier_rank, v.tagline, v.features, v.price_micros, v.price_yearly_micros,
       30, v.quota_micros, v.forge_quota_micros, v.highlight, TRUE
FROM (VALUES
    ('Hobby', '免费方案：每月少量平台模型额度。', 'hobby', 0, '适合随手试试',
     ARRAY['无需绑卡', '每月少量平台模型额度', '设置、记忆与个人技能云同步'],
     0::bigint, 0::bigint, 0::bigint, 1000000::bigint, FALSE),
    ('Pro', '每月附带第三方前沿模型与平台模型额度。', 'pro', 10, '适合开始使用 Agent 的开发者',
     ARRAY['包含 Hobby 全部权益', '可用第三方前沿模型（按 API 价计入额度）', '套餐内额度每月重置', '额度用完可开启按量付费'],
     20000000::bigint, 192000000::bigint, 20000000::bigint, 60000000::bigint, FALSE),
    ('Pro+', '更高的每月额度，适合每天使用 Agent。', 'pro_plus', 20, '适合每天使用 Agent 的开发者',
     ARRAY['包含 Pro 全部权益', '平台模型额度为 Pro 的 3 倍', '第三方模型额度 70 美元/月'],
     60000000::bigint, 576000000::bigint, 70000000::bigint, 180000000::bigint, TRUE),
    ('Ultra', '最高额度，适合多 Agent 并行与自动化。', 'ultra', 30, '适合重度 Agent 用户',
     ARRAY['包含 Pro+ 全部权益', '平台模型额度为 Pro 的 20 倍', '第三方模型额度 400 美元/月'],
     200000000::bigint, 1920000000::bigint, 400000000::bigint, 1200000000::bigint, FALSE)
) AS v(name, description, tier, tier_rank, tagline, features, price_micros, price_yearly_micros,
       quota_micros, forge_quota_micros, highlight)
WHERE NOT EXISTS (SELECT 1 FROM plans p WHERE p.tier = v.tier);

-- +goose Down
DROP INDEX IF EXISTS payment_orders_user;
ALTER TABLE payment_orders
    DROP COLUMN IF EXISTS paid_at,
    DROP COLUMN IF EXISTS note,
    DROP COLUMN IF EXISTS pay_url,
    DROP COLUMN IF EXISTS subscription_id,
    DROP COLUMN IF EXISTS replaces_subscription_id,
    DROP COLUMN IF EXISTS credit_micros,
    DROP COLUMN IF EXISTS list_price_micros,
    DROP COLUMN IF EXISTS mode,
    DROP COLUMN IF EXISTS billing_interval,
    DROP COLUMN IF EXISTS plan_id,
    DROP COLUMN IF EXISTS kind;
DROP INDEX IF EXISTS usage_logs_user_on_demand;
ALTER TABLE usage_logs DROP COLUMN IF EXISTS pool;
ALTER TABLE users
    DROP COLUMN IF EXISTS free_forge_used_micros,
    DROP COLUMN IF EXISTS free_api_used_micros,
    DROP COLUMN IF EXISTS free_cycle_start,
    DROP COLUMN IF EXISTS on_demand_limit_micros,
    DROP COLUMN IF EXISTS on_demand_enabled;
ALTER TABLE subscriptions
    DROP COLUMN IF EXISTS order_id,
    DROP COLUMN IF EXISTS source,
    DROP COLUMN IF EXISTS value_micros,
    DROP COLUMN IF EXISTS billing_interval,
    DROP COLUMN IF EXISTS cycle_start,
    DROP COLUMN IF EXISTS usage_cycle,
    DROP COLUMN IF EXISTS forge_used_micros,
    DROP COLUMN IF EXISTS forge_quota_micros;
ALTER TABLE models DROP COLUMN IF EXISTS pool;
DELETE FROM plans p WHERE p.tier IN ('hobby', 'pro', 'pro_plus', 'ultra')
    AND NOT EXISTS (SELECT 1 FROM subscriptions s WHERE s.plan_id = p.id)
    AND NOT EXISTS (SELECT 1 FROM redeem_codes c WHERE c.plan_id = p.id);
DROP INDEX IF EXISTS plans_tier;
ALTER TABLE plans
    DROP COLUMN IF EXISTS highlight,
    DROP COLUMN IF EXISTS forge_quota_micros,
    DROP COLUMN IF EXISTS price_yearly_micros,
    DROP COLUMN IF EXISTS features,
    DROP COLUMN IF EXISTS tagline,
    DROP COLUMN IF EXISTS tier_rank,
    DROP COLUMN IF EXISTS tier;
