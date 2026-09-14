//! Fictional V6 gameplay values; real GPU model names are not benchmark claims.
use serde::{Deserialize, Serialize};
pub const BRANCHES: [&str; 5] = ["speed", "security", "algorithm", "science", "lightweight"];
pub const STARTING_CREDITS: f64 = 2000.;
pub const AI_PURCHASE_GROWTH: f64 = 0.12;
pub const AI_UPKEEP_QUADRATIC: f64 = 0.35;
pub const AI_DEPLOY_COMPUTE: f64 = 120.;
pub const GEMINI_TELEGRAPH_SECONDS: f64 = 2.2;
pub const ELEVATOR_POWER_DEMAND: f64 = 8.;
pub const SHELL_CELL_COST: f64 = 6.;
pub const SHELL_EDGE_COST: f64 = 2.;
pub const ROOM_CELL_COST: f64 = 3.;
pub const TRANSPORT_DISPATCH_COST: f64 = 6.;
pub const TRANSPORT_REROUTE_COST: f64 = 2.;
pub const AIR_TRANSPORT_DISPATCH_COST: f64 = 80.;
pub const AIR_TRANSPORT_FUEL_MAX: f64 = 40.;
pub const AIR_TRANSPORT_FUEL_PER_CELL: f64 = 0.06;
pub const AIR_TRANSPORT_PHASE_SECONDS: f64 = 2.;
pub const AIR_TRANSPORT_SPEED: f64 = 8.;
pub const MIN_SHIPMENT_AMOUNT: f64 = 1.;
pub const RESEARCH_CREDITS: [f64; 5] = [100., 350., 700., 1400., 2400.];
pub const RESEARCH_COMPUTE: [f64; 5] = [60., 150., 400., 900., 1800.];
pub const RESEARCH_DATA: [f64; 5] = [0., 0., 60., 180., 360.];
pub const RESEARCH_SECONDS: [f64; 5] = [35., 60., 100., 160., 220.];
pub const RESEARCH_ADDITIONAL_BRANCH_FACTOR: f64 = 0.8;
pub const RESEARCH_ADDITIONAL_BRANCH_FROM_TIER: u32 = 3;
pub fn research_multiplier(
    branches: &std::collections::BTreeMap<String, u32>,
    branch: &str,
) -> f64 {
    1. + RESEARCH_ADDITIONAL_BRANCH_FACTOR
        * branches
            .iter()
            .filter(|(other, tier)| {
                other.as_str() != branch && **tier >= RESEARCH_ADDITIONAL_BRANCH_FROM_TIER
            })
            .count() as f64
}
pub const LAB_COMPUTE_UPKEEP: [f64; 5] = [0.5, 1.5, 3., 6., 10.];
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitDef {
    pub id: String,
    pub name: String,
    pub branch: String,
    pub tier: u32,
    pub category: String,
    pub cost: f64,
    pub hp: f64,
    pub damage: f64,
    pub range: f64,
    pub period: f64,
    pub speed: f64,
    pub trajectory: String,
    pub damage_type: String,
    pub radius: f64,
    pub skill: String,
    pub description: String,
    pub chassis: String,
    pub attack_pattern: String,
    pub target_air: bool,
    pub target_ground: bool,
    pub breach_depth: u32,
    pub structure_multiplier: f64,
    pub ammo_capacity: f64,
    pub ammo_per_shot: f64,
    pub fuel_capacity: f64,
    pub compute_per_attack: f64,
    pub energy_per_attack: f64,
    pub energy_capacity: f64,
    pub skill_cost: f64,
    pub skill_cooldown: f64,
    pub skill_range: f64,
    pub skill_shape: String,
    pub skill_radius: f64,
    pub skill_angle: f64,
    pub skill_width: f64,
    pub command_cost: u32,
    pub passive: Option<PassiveDef>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PassiveDef {
    pub id: String,
    pub name: String,
    pub description: String,
}
fn ai_passive(id: &str) -> Option<PassiveDef> {
    let (id, name, description) = match id {
        "kimi" => ("pre-read", "预读侦察", "基础视野26格，保持墙体与楼板遮挡"),
        "claude" => (
            "guardian-intercept",
            "守护拦截",
            "每8秒最多支付8算力，将6格内瞄准同层友军或运输的制导弹载荷削弱20%",
        ),
        "gpt" => (
            "maintenance-daemon",
            "维护守护",
            "联网时每6秒最多支付6算力，维修6格内一名同层可见受损友军12生命，受减疗影响",
        ),
        "deepseek" => ("trace-mark", "推理追踪", "普攻真实命中后施加3秒标记"),
        "gemini" => (
            "triad-ready",
            "三轮协同",
            "每3轮成功普攻获得20秒就绪，下一次成功主动技能费用降低10%",
        ),
        "minimax" => (
            "suppressive-pulse",
            "抑制脉冲",
            "每第4轮普攻的实际命中对单位与建筑施加3秒减疗",
        ),
        "glm" => (
            "light-cache",
            "轻量缓存",
            "基础随身缓存上限300；初始算力仍为120",
        ),
        _ => return None,
    };
    Some(PassiveDef {
        id: id.into(),
        name: name.into(),
        description: description.into(),
    })
}
pub const FACILITY_BONUS_RADIUS: f64 = 12.;
pub fn facility_bonus(kind: &str) -> f64 {
    match kind {
        "rapid-logistics" => 0.25,
        "secure-relay" => 0.35,
        "targeting-array" | "particle-foundry" => 0.2,
        "modular-workshop" => 0.1,
        _ => 0.,
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuDef {
    pub id: String,
    pub name: String,
    pub tier: u32,
    pub cost: f64,
    pub power: f64,
    pub rate: f64,
    pub capacity: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FacilityDef {
    pub id: String,
    pub name: String,
    pub cost: f64,
    pub power: f64,
    pub capacity_per_area: f64,
    pub branch: String,
    pub tier: u32,
    pub description: String,
    pub min_area: u32,
    pub upkeep_compute: f64,
    pub stock_per_area: f64,
    pub bonus: f64,
    pub bonus_radius: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutdoorDef {
    pub id: String,
    pub name: String,
    pub cost: f64,
    pub power: f64,
    pub tier: u32,
    pub width: u32,
    pub height: u32,
    pub description: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDef {
    pub id: String,
    pub name: String,
    pub branch: String,
    pub category: String,
    pub tier: u32,
    pub cost: f64,
    pub compute_cost: f64,
    pub modifier: String,
    pub magnitude: f64,
    pub description: String,
}
pub fn gpus() -> Vec<GpuDef> {
    [
        ("rtx-5060", "RTX 5060", 1, 130., 55., 12., 300.),
        ("rtx-5070", "RTX 5070", 2, 220., 85., 20., 450.),
        ("rtx-5080", "RTX 5080", 3, 350., 120., 32., 700.),
        ("rtx-5090", "RTX 5090", 4, 550., 180., 49., 1000.),
        ("rtx-pro-6000", "RTX PRO 6000", 4, 820., 250., 70., 1400.),
        ("a100", "A100", 3, 780., 220., 60., 1600.),
        ("h200", "H200", 5, 1500., 400., 110., 2200.),
    ]
    .into_iter()
    .map(|(id, name, tier, cost, power, rate, capacity)| GpuDef {
        id: id.into(),
        name: name.into(),
        tier,
        cost,
        power,
        rate,
        capacity,
    })
    .collect()
}
pub fn facilities() -> Vec<FacilityDef> {
    [
        (
            "corridor",
            "通道",
            4.,
            0.,
            0.,
            "",
            1,
            1,
            0.,
            "占用真实通行面积，不产生容量或资源",
        ),
        (
            "column",
            "承重柱",
            12.,
            0.,
            0.,
            "",
            1,
            1,
            0.,
            "占用楼层净面积，提供局部结构承重",
        ),
        (
            "drainage",
            "排水房",
            220.,
            35.,
            0.,
            "",
            2,
            4,
            0.,
            "通电后排除作业区域地下积水",
        ),
        (
            "drill-workshop",
            "钻探房",
            260.,
            40.,
            0.,
            "",
            2,
            4,
            0.,
            "通电钻探设备允许破除地下岩层",
        ),
        (
            "data-synthesis",
            "科研数据合成室",
            300.,
            35.,
            0.,
            "",
            2,
            4,
            6.,
            "低效合成科研数据：消耗经费和本地算力，作为封锁后的恢复路径",
        ),
        (
            "wireless-relay",
            "楼层无线接入室",
            160.,
            10.,
            0.,
            "",
            2,
            4,
            0.5,
            "需本层有线数据中心回传，提供12格同层无线覆盖；不能跨楼板供给",
        ),
        (
            "data-center",
            "数据中心",
            120.,
            20.,
            0.25,
            "",
            1,
            4,
            0.,
            "净面积每4格一个机架；本地电力、算力库存隔离",
        ),
        (
            "research-lab",
            "研究所",
            100.,
            20.,
            0.,
            "",
            1,
            4,
            0.5,
            "每所一个方向；可多所并行研究，建成初始所解锁基础炮塔",
        ),
        (
            "depot",
            "补给仓库",
            120.,
            10.,
            20.,
            "",
            1,
            4,
            0.,
            "接收真实货物，储存弹药、燃料和维修材料",
        ),
        (
            "ammunition-workshop",
            "弹药车间",
            180.,
            25.,
            10.,
            "",
            1,
            4,
            0.,
            "支付原料制造弹药，须实际运抵作战单位",
        ),
        (
            "factory",
            "车辆工厂",
            240.,
            40.,
            0.5,
            "",
            1,
            8,
            0.,
            "共享陆军底盘；大型车辆需要宽出口和坡道",
        ),
        (
            "airfield",
            "航空整备站",
            420.,
            55.,
            0.2,
            "",
            2,
            12,
            0.,
            "所有分支T2开放，须连接露天起降区",
        ),
        (
            "anti-air-control",
            "防空指挥室",
            180.,
            25.,
            0.,
            "",
            2,
            4,
            0.,
            "各分支基础对空反制的控制设备",
        ),
        (
            "network-defense",
            "网络防御站",
            200.,
            10.,
            8.,
            "",
            1,
            4,
            1.,
            "接本地数据中心；有限算力抵挡引导和网络攻击",
        ),
        (
            "energy-defense",
            "能量防御站",
            240.,
            60.,
            8.,
            "",
            2,
            4,
            0.,
            "接发电网络的有限充能护盾",
        ),
        (
            "repair-bay",
            "维修站",
            180.,
            25.,
            0.,
            "",
            2,
            6,
            0.,
            "消耗本地维修材料，运输路线必须可达",
        ),
        (
            "missile-silo",
            "导弹控制室",
            700.,
            90.,
            0.,
            "",
            4,
            8,
            0.,
            "各分支远程弹药与发射平台控制设施",
        ),
        (
            "orbital-control",
            "天基控制中心",
            1200.,
            160.,
            0.,
            "",
            5,
            12,
            8.,
            "可摧毁的地面控制中心；真实准备和补给",
        ),
        (
            "rapid-logistics",
            "快速补给中枢",
            300.,
            35.,
            12.,
            "speed",
            2,
            6,
            2.,
            "有效同层算力组件内，12格装卸速率提高25%；运输仍须实际通行",
        ),
        (
            "secure-relay",
            "安全交换中枢",
            350.,
            40.,
            0.,
            "security",
            3,
            6,
            3.,
            "有效同层算力组件内，12格抗制导干扰提高35%",
        ),
        (
            "targeting-array",
            "预测火控室",
            350.,
            45.,
            0.,
            "algorithm",
            3,
            6,
            3.,
            "有效同层算力组件内，12格预测上限提高20%，对已标记目标普攻额外加成5%",
        ),
        (
            "particle-foundry",
            "粒子实验室",
            650.,
            120.,
            0.,
            "science",
            4,
            8,
            6.,
            "有效同层算力组件内，12格充能速率提高20%，仍消耗真实电网余量",
        ),
        (
            "modular-workshop",
            "轻量模块工坊",
            230.,
            20.,
            8.,
            "lightweight",
            2,
            4,
            1.,
            "有效同层算力组件内，12格插件安装经费降低10%，不减免算力费用",
        ),
    ]
    .into_iter()
    .map(
        |(
            id,
            name,
            cost,
            power,
            capacity_per_area,
            branch,
            tier,
            min_area,
            upkeep_compute,
            description,
        )| FacilityDef {
            id: id.into(),
            name: name.into(),
            cost,
            power,
            capacity_per_area,
            branch: branch.into(),
            tier,
            min_area,
            upkeep_compute,
            stock_per_area: match id {
                "depot" | "airfield" | "orbital-control" | "factory" => 20.,
                "ammunition-workshop" | "modular-workshop" | "rapid-logistics" => 10.,
                "repair-bay" => 15.,
                _ => 0.,
            },
            bonus: facility_bonus(id),
            bonus_radius: if facility_bonus(id) > 0. {
                FACILITY_BONUS_RADIUS
            } else {
                0.
            },
            description: description.into(),
        },
    )
    .collect()
}
pub fn outdoor() -> Vec<OutdoorDef> {
    [
        (
            "wind-power",
            "风力发电",
            160.,
            150.,
            1,
            2,
            2,
            "价格包含基础框架；高地增产，接电力线供电",
        ),
        (
            "hydro-power",
            "水力发电",
            380.,
            450.,
            2,
            3,
            3,
            "河岸基础设施；地形与线路必须有效",
        ),
        (
            "coal-power",
            "煤炭火电",
            600.,
            950.,
            2,
            3,
            3,
            "使用运抵煤炭；断供后库存耗尽停机",
        ),
        (
            "nuclear-power",
            "核电站",
            1600.,
            3000.,
            4,
            4,
            4,
            "需要冷却水和实体燃料",
        ),
        (
            "extractor",
            "资源采集器",
            180.,
            0.,
            1,
            2,
            2,
            "有限矿量先成为货物，运抵仓库结算经费",
        ),
        (
            "mobile-relay",
            "移动算力基站",
            280.,
            10.,
            1,
            2,
            2,
            "消耗10电力并依赖数据中心回传，离网AI使用随身缓存",
        ),
        (
            "airstrip",
            "露天机场",
            360.,
            20.,
            2,
            8,
            4,
            "净空跑道和起降路线；毁坏会中断航空补给",
        ),
        (
            "launch-pad",
            "发射平台",
            800.,
            50.,
            4,
            4,
            4,
            "露天导弹与航天发射基础设施",
        ),
    ]
    .into_iter()
    .map(
        |(id, name, cost, power, tier, width, height, description)| OutdoorDef {
            id: id.into(),
            name: name.into(),
            cost,
            power,
            tier,
            width,
            height,
            description: description.into(),
        },
    )
    .collect()
}
struct Chassis {
    id: &'static str,
    name: &'static str,
    tier: u32,
    category: &'static str,
    cost: f64,
    trajectory: &'static str,
    damage_type: &'static str,
    period: f64,
    range: f64,
    radius: f64,
    air: bool,
    ground: bool,
    breach: u32,
    structure: f64,
}
fn chassis() -> Vec<Chassis> {
    [
        (
            "machinegun",
            "机枪炮塔",
            1,
            "turret",
            120.,
            "direct",
            "kinetic",
            0.8,
            12.,
            0.,
            true,
            true,
            0,
            0.75,
        ),
        (
            "light-mortar",
            "轻迫击炮",
            1,
            "turret",
            140.,
            "arc",
            "explosive",
            2.4,
            17.,
            2.,
            false,
            true,
            0,
            1.35,
        ),
        (
            "scout",
            "侦察车",
            1,
            "vehicle",
            120.,
            "direct",
            "kinetic",
            1.,
            11.,
            0.,
            true,
            true,
            0,
            0.8,
        ),
        (
            "tank",
            "主战坦克",
            2,
            "vehicle",
            260.,
            "direct",
            "kinetic",
            1.6,
            14.,
            0.,
            false,
            true,
            0,
            1.25,
        ),
        (
            "aa-launcher",
            "防空导弹",
            2,
            "turret",
            240.,
            "guided",
            "explosive",
            1.8,
            19.,
            0.6,
            true,
            true,
            0,
            0.55,
        ),
        (
            "breacher",
            "破障工程车",
            2,
            "vehicle",
            230.,
            "direct",
            "explosive",
            2.5,
            7.,
            1.4,
            false,
            true,
            1,
            1.65,
        ),
        (
            "artillery",
            "自行火炮",
            3,
            "vehicle",
            520.,
            "arc",
            "explosive",
            3.,
            27.,
            3.,
            false,
            true,
            1,
            1.45,
        ),
        (
            "attack-aircraft",
            "攻击机",
            3,
            "air",
            570.,
            "guided",
            "kinetic",
            1.3,
            16.,
            0.8,
            true,
            true,
            0,
            1.,
        ),
        (
            "rail-accelerator",
            "轨道加速炮",
            3,
            "turret",
            540.,
            "direct",
            "kinetic",
            2.4,
            24.,
            0.,
            true,
            true,
            1,
            1.3,
        ),
        (
            "particle-cannon",
            "粒子炮",
            4,
            "turret",
            980.,
            "beam",
            "energy",
            2.6,
            26.,
            0.,
            true,
            true,
            1,
            1.4,
        ),
        (
            "missile-truck",
            "远程导弹车",
            4,
            "vehicle",
            1020.,
            "arc",
            "explosive",
            4.,
            38.,
            3.5,
            false,
            true,
            2,
            1.4,
        ),
        (
            "bomber",
            "先进轰炸机",
            4,
            "air",
            1080.,
            "arc",
            "explosive",
            3.5,
            15.,
            3.5,
            false,
            true,
            1,
            1.3,
        ),
        (
            "aerospace",
            "空天战机",
            5,
            "air",
            1800.,
            "guided",
            "energy",
            1.6,
            24.,
            1.,
            true,
            true,
            1,
            1.,
        ),
        (
            "orbital-strike",
            "天基打击平台",
            5,
            "orbital",
            2200.,
            "orbital",
            "energy",
            10.,
            90.,
            5.,
            false,
            true,
            2,
            1.5,
        ),
        (
            "anti-orbital",
            "反轨道防御",
            5,
            "turret",
            1700.,
            "guided",
            "energy",
            2.6,
            34.,
            1.2,
            true,
            true,
            0,
            0.65,
        ),
    ]
    .into_iter()
    .map(
        |(
            id,
            name,
            tier,
            category,
            cost,
            trajectory,
            damage_type,
            period,
            range,
            radius,
            air,
            ground,
            breach,
            structure,
        )| Chassis {
            id,
            name,
            tier,
            category,
            cost,
            trajectory,
            damage_type,
            period,
            range,
            radius,
            air,
            ground,
            breach,
            structure,
        },
    )
    .collect()
}
fn branch_name(branch: &str) -> &str {
    match branch {
        "speed" => "疾速",
        "security" => "安全",
        "algorithm" => "算法",
        "science" => "科研",
        "lightweight" => "轻量",
        _ => "通用",
    }
}
fn variant_id(branch: &str, chassis: &str) -> String {
    let alias = match (branch, chassis) {
        ("speed", "machinegun") => Some("autocannon"),
        ("speed", "tank") => Some("light-tank"),
        ("speed", "attack-aircraft") => Some("fighter"),
        ("speed", "aerospace") => Some("aerospace-fighter"),
        ("security", "machinegun") => Some("interceptor"),
        ("security", "tank") => Some("shield-tank"),
        ("security", "aa-launcher") => Some("aa-turret"),
        ("security", "anti-orbital") => Some("aegis-array"),
        ("algorithm", "light-mortar") => Some("mortar"),
        ("algorithm", "artillery") => Some("artillery"),
        ("algorithm", "missile-truck") => Some("missile-truck"),
        ("science", "machinegun") => Some("pulse-turret"),
        ("science", "tank") => Some("laser-tank"),
        ("science", "artillery") => Some("plasma-cannon"),
        ("science", "particle-cannon") => Some("particle-cannon"),
        ("science", "orbital-strike") => Some("orbital-lance"),
        ("lightweight", "scout") => Some("scout-buggy"),
        ("lightweight", "attack-aircraft") => Some("loiter-drone"),
        ("lightweight", "bomber") => Some("stealth-wing"),
        ("lightweight", "aerospace") => Some("distributed-array"),
        _ => None,
    };
    alias
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{branch}-{chassis}"))
}
fn build_units() -> Vec<UnitDef> {
    let mut out = Vec::new();
    for (id, name, cost, damage, period, radius, pattern, skill, shape) in [
        (
            "vscode",
            "VS Code",
            70.,
            24.,
            0.8,
            0.,
            "single",
            "precision",
            "target",
        ),
        (
            "pycharm",
            "PyCharm",
            80.,
            22.,
            1.25,
            2.,
            "cone",
            "debug-cone",
            "cone",
        ),
    ] {
        out.push(UnitDef {
            id: id.into(),
            name: name.into(),
            branch: String::new(),
            tier: 1,
            category: "turret".into(),
            cost,
            hp: 260.,
            damage,
            range: 12.,
            period,
            speed: 0.,
            trajectory: "direct".into(),
            damage_type: "kinetic".into(),
            radius,
            skill: skill.into(),
            description: "初始研究所建成后解锁；固定驻守，接本地算力网络".into(),
            chassis: id.into(),
            attack_pattern: pattern.into(),
            target_air: true,
            target_ground: true,
            breach_depth: 0,
            structure_multiplier: 0.8,
            ammo_capacity: 0.,
            ammo_per_shot: 0.,
            fuel_capacity: 0.,
            compute_per_attack: 2.,
            energy_per_attack: 0.,
            energy_capacity: 0.,
            skill_cost: 55.,
            skill_cooldown: 18.,
            skill_range: 14.,
            skill_shape: shape.into(),
            skill_radius: if shape == "cone" { 6. } else { 0. },
            skill_angle: if shape == "cone" { 60. } else { 0. },
            skill_width: 0.,
            command_cost: 2,
            passive: None,
        });
    }
    for (
        id,
        name,
        branch,
        tier,
        cost,
        damage,
        pattern,
        skill,
        shape,
        skill_cost,
        cooldown,
        description,
    ) in [
        (
            "kimi",
            "Kimi",
            "speed",
            2,
            780.,
            48.,
            "burst",
            "dash-strike",
            "direction",
            100.,
            18.,
            "单体爆发，定向突进斩击",
        ),
        (
            "claude",
            "Claude Sonnet",
            "security",
            2,
            800.,
            40.,
            "single",
            "intercept-barrier",
            "cone",
            105.,
            22.,
            "精确攻击，锥形拦截屏障",
        ),
        (
            "gpt",
            "GPT",
            "security",
            3,
            1500.,
            54.,
            "single",
            "repair-armor",
            "circle-ally",
            150.,
            25.,
            "单体射击，区域维修与护甲增益",
        ),
        (
            "deepseek",
            "DeepSeek",
            "algorithm",
            2,
            800.,
            34.,
            "chain",
            "piercing-mark",
            "direction",
            110.,
            21.,
            "链式普攻，定向贯穿与标记",
        ),
        (
            "gemini",
            "Gemini",
            "science",
            2,
            840.,
            48.,
            "dual-beam",
            "telegraphed-bombardment",
            "circle",
            120.,
            25.,
            "双束普攻，预警区域轰击",
        ),
        (
            "minimax",
            "MiniMax",
            "science",
            3,
            1600.,
            50.,
            "cone",
            "repair-heal-cut",
            "circle-mixed",
            155.,
            26.,
            "扇形冲击波，范围修复和减疗",
        ),
        (
            "glm",
            "GLM",
            "lightweight",
            2,
            660.,
            38.,
            "single",
            "targeted-support",
            "target-ally",
            85.,
            18.,
            "低成本单体射击，指定友军支援",
        ),
    ] {
        out.push(UnitDef {
            id: id.into(),
            name: name.into(),
            branch: branch.into(),
            tier,
            category: "ai".into(),
            cost,
            hp: 420. * 1.4_f64.powi(tier as i32 - 2),
            damage,
            range: if id == "kimi" { 16. } else { 14. },
            period: 1.2,
            speed: if branch == "speed" { 4.8 } else { 3.8 },
            trajectory: if id == "gemini" { "beam" } else { "direct" }.into(),
            damage_type: if branch == "science" {
                "energy"
            } else {
                "network"
            }
            .into(),
            radius: if pattern == "cone" { 3. } else { 0. },
            skill: skill.into(),
            description: description.into(),
            chassis: id.into(),
            attack_pattern: pattern.into(),
            target_air: true,
            target_ground: true,
            breach_depth: 0,
            structure_multiplier: 0.65,
            ammo_capacity: 0.,
            ammo_per_shot: 0.,
            fuel_capacity: 0.,
            compute_per_attack: if branch == "lightweight" { 4. } else { 6. },
            energy_per_attack: 0.,
            energy_capacity: 0.,
            skill_cost,
            skill_cooldown: cooldown,
            skill_range: match id {
                "kimi" => 10.,
                "claude" => 10.,
                "gemini" => 24.,
                "minimax" => 14.,
                _ => 18.,
            },
            skill_shape: shape.into(),
            skill_radius: if shape.starts_with("circle") {
                4.
            } else if shape == "cone" {
                8.
            } else {
                0.
            },
            skill_angle: if shape == "cone" { 70. } else { 0. },
            skill_width: if shape == "direction" { 1.5 } else { 0. },
            command_cost: 12,
            passive: ai_passive(id),
        });
    }
    for branch in BRANCHES {
        for base in chassis() {
            let scale = 1.4_f64.powi(base.tier as i32 - 1);
            let (cf, hf, pf, sf, df) = match branch {
                "speed" => (1., 0.85, 1. / 1.2, 1.2, 1.),
                "security" => (1., 1.12, 1., 0.95, 0.9),
                "algorithm" => (1.05, 1., 1., 1., 1.),
                "science" => (1.1, 0.95, 1.15, 0.9, 1.15),
                "lightweight" => (0.85, 0.8, 1., 1.08, 0.9),
                _ => (1., 1., 1., 1., 1.),
            };
            let period = base.period * pf;
            let damage = 20. * scale * base.period * df * if base.radius >= 2. { 0.6 } else { 1. };
            let energy = base.damage_type == "energy"
                || branch == "science" && matches!(base.id, "machinegun" | "tank");
            let ammo_per_shot = if energy { 0. } else { 1. };
            let ammo_capacity = if energy { 0. } else { (35. / period).ceil() };
            out.push(UnitDef {
                id: variant_id(branch, base.id),
                name: format!("{}{}", branch_name(branch), base.name),
                branch: branch.into(),
                tier: base.tier,
                category: base.category.into(),
                cost: if branch == "science" && base.id == "scout" {
                    126.
                } else {
                    (base.cost * cf).round()
                },
                hp: 300. * scale * hf,
                damage,
                range: base.range,
                period,
                speed: if matches!(base.category, "turret" | "orbital") {
                    0.
                } else if base.category == "air" {
                    7. * sf
                } else if branch == "science" && base.id == "scout" {
                    3.
                } else {
                    3. * sf
                },
                trajectory: if energy && base.trajectory == "direct" && branch == "science" {
                    "beam"
                } else {
                    base.trajectory
                }
                .into(),
                damage_type: if energy { "energy" } else { base.damage_type }.into(),
                radius: base.radius,
                skill: match branch {
                    "speed" => "overclock",
                    "security" => "guard",
                    "algorithm" => "target-lock",
                    "science" => "charged-shot",
                    _ => "field-resupply",
                }
                .into(),
                description: format!(
                    "{}分支T{}；本地弹药/能量库存，真实命中结算",
                    branch_name(branch),
                    base.tier
                ),
                chassis: base.id.into(),
                attack_pattern: if base.trajectory == "arc" {
                    "area"
                } else {
                    "single"
                }
                .into(),
                target_air: base.air,
                target_ground: base.ground,
                breach_depth: base.breach,
                structure_multiplier: base.structure,
                ammo_capacity,
                ammo_per_shot,
                fuel_capacity: if matches!(base.category, "vehicle" | "air") {
                    120.
                } else {
                    0.
                },
                compute_per_attack: 0.,
                energy_per_attack: if energy { 10. * base.tier as f64 } else { 0. },
                energy_capacity: if energy {
                    (35. / period).ceil() * 10. * base.tier as f64
                } else {
                    0.
                },
                skill_cost: 35. + base.tier as f64 * 15.,
                skill_cooldown: 25.,
                skill_range: base.range,
                skill_shape: if branch == "algorithm" {
                    "target"
                } else {
                    "self"
                }
                .into(),
                skill_radius: 0.,
                skill_angle: 0.,
                skill_width: 0.,
                command_cost: if base.tier >= 4 { 6 } else { 4 },
                passive: None,
            });
        }
    }
    out
}
fn build_plugins() -> Vec<PluginDef> {
    let mut out = vec![];
    for branch in BRANCHES {
        for (category, tier, cost, compute_cost) in [
            ("core", 2, 220., 60.),
            ("attack", 3, 400., 100.),
            ("support", 4, 650., 160.),
        ] {
            let (modifier, magnitude, description) = match (branch, category) {
                ("speed", "core") => ("move-speed", 0.2, "提高移动速度"),
                ("speed", "attack") => ("attack-rate", 0.15, "提高射速和对应补给消耗"),
                ("speed", _) => ("reload-rate", 0.2, "提高近场装卸效率"),
                ("security", "core") => (
                    "jam-resistance",
                    0.35,
                    "降低制导偏移35%，本机承受的网络攻击伤害降低17.5%",
                ),
                ("security", "attack") => ("interception", 0.2, "提高可拦截弹道处理效率"),
                ("security", _) => ("ally-armor", 0.15, "近场友军护甲支援"),
                ("algorithm", "core") => ("prediction", 0.2, "提高移动目标预测能力"),
                ("algorithm", "attack") => ("mark-damage", 0.2, "提高标记目标伤害"),
                ("algorithm", _) => ("vision-range", 0.15, "扩展本楼层侦察范围"),
                ("science", "core") => (
                    "battery-capacity",
                    0.2,
                    "已有缓存和蓄能上限提高20%；无缓存机械新增120空缓存上限",
                ),
                ("science", "attack") => ("skill-power", 0.2, "增加技能效果和对应能耗"),
                ("science", _) => ("skill-cooldown", 0.15, "缩短准备冷却"),
                ("lightweight", "core") => ("compute-efficiency", 0.15, "降低本机普攻算力消耗"),
                ("lightweight", "attack") => (
                    "payload-efficiency",
                    0.15,
                    "普攻弹药、能量和算力消耗各降低15%",
                ),
                _ => ("repair-efficiency", 0.2, "提高真实维修物资利用效率"),
            };
            out.push(PluginDef {
                id: format!("{branch}-{category}"),
                name: format!(
                    "{}{}插件",
                    branch_name(branch),
                    match category {
                        "core" => "核心",
                        "attack" => "攻击",
                        _ => "支援",
                    }
                ),
                branch: branch.into(),
                category: category.into(),
                tier,
                cost,
                compute_cost,
                modifier: modifier.into(),
                magnitude,
                description: description.into(),
            });
        }
    }
    out
}
fn unit_defs() -> &'static Vec<UnitDef> {
    static UNITS: std::sync::OnceLock<Vec<UnitDef>> = std::sync::OnceLock::new();
    UNITS.get_or_init(build_units)
}
pub fn units() -> Vec<UnitDef> {
    unit_defs().clone()
}
pub fn unit(id: &str) -> Option<UnitDef> {
    unit_ref(id).cloned()
}
pub fn unit_ref(id: &str) -> Option<&'static UnitDef> {
    static INDEX: std::sync::OnceLock<std::collections::HashMap<String, usize>> =
        std::sync::OnceLock::new();
    INDEX
        .get_or_init(|| {
            unit_defs()
                .iter()
                .enumerate()
                .map(|(i, d)| (d.id.clone(), i))
                .collect()
        })
        .get(id)
        .map(|i| &unit_defs()[*i])
}
fn plugin_defs() -> &'static Vec<PluginDef> {
    static DEFS: std::sync::OnceLock<Vec<PluginDef>> = std::sync::OnceLock::new();
    DEFS.get_or_init(build_plugins)
}
pub fn plugins() -> Vec<PluginDef> {
    plugin_defs().clone()
}
pub fn plugin(id: &str) -> Option<PluginDef> {
    plugin_ref(id).cloned()
}
pub fn plugin_ref(id: &str) -> Option<&'static PluginDef> {
    static INDEX: std::sync::OnceLock<std::collections::HashMap<String, usize>> =
        std::sync::OnceLock::new();
    INDEX
        .get_or_init(|| {
            plugin_defs()
                .iter()
                .enumerate()
                .map(|(i, d)| (d.id.clone(), i))
                .collect()
        })
        .get(id)
        .map(|i| &plugin_defs()[*i])
}
pub fn facility(id: &str) -> Option<FacilityDef> {
    facility_ref(id).cloned()
}
fn facility_defs() -> &'static Vec<FacilityDef> {
    static DEFS: std::sync::OnceLock<Vec<FacilityDef>> = std::sync::OnceLock::new();
    DEFS.get_or_init(facilities)
}
pub fn facility_ref(id: &str) -> Option<&'static FacilityDef> {
    static INDEX: std::sync::OnceLock<std::collections::HashMap<String, usize>> =
        std::sync::OnceLock::new();
    INDEX
        .get_or_init(|| {
            facility_defs()
                .iter()
                .enumerate()
                .map(|(i, d)| (d.id.clone(), i))
                .collect()
        })
        .get(id)
        .map(|i| &facility_defs()[*i])
}
pub fn outdoor_building(id: &str) -> Option<OutdoorDef> {
    outdoor_ref(id).cloned()
}
fn outdoor_defs() -> &'static Vec<OutdoorDef> {
    static DEFS: std::sync::OnceLock<Vec<OutdoorDef>> = std::sync::OnceLock::new();
    DEFS.get_or_init(outdoor)
}
pub fn outdoor_ref(id: &str) -> Option<&'static OutdoorDef> {
    static INDEX: std::sync::OnceLock<std::collections::HashMap<String, usize>> =
        std::sync::OnceLock::new();
    INDEX
        .get_or_init(|| {
            outdoor_defs()
                .iter()
                .enumerate()
                .map(|(i, d)| (d.id.clone(), i))
                .collect()
        })
        .get(id)
        .map(|i| &outdoor_defs()[*i])
}
pub fn catalog() -> serde_json::Value {
    serde_json::json!({"version":6,"rulesVersion":crate::RULES_VERSION,"rulesFingerprint":crate::RULES_FINGERPRINT,"logistics":{"dispatchCost":TRANSPORT_DISPATCH_COST,"rerouteCost":TRANSPORT_REROUTE_COST,"airDispatchCost":AIR_TRANSPORT_DISPATCH_COST,"airFuelMax":AIR_TRANSPORT_FUEL_MAX,"airFuelPerCell":AIR_TRANSPORT_FUEL_PER_CELL,"airPhaseSeconds":AIR_TRANSPORT_PHASE_SECONDS,"airSpeed":AIR_TRANSPORT_SPEED,"airHp":260,"minimumAmount":MIN_SHIPMENT_AMOUNT,"courierBaseHp":60,"courierHpPerSqrtAmount":8},"branches":[{"id":"speed","name":"速度","trait":"机动、射速、快速补给；耐久较低"},{"id":"security","name":"网安","trait":"拦截、抗干扰、补给保护；持续伤害较低"},{"id":"algorithm","name":"算法","trait":"预测、标记、贯穿和目标选择"},{"id":"science","name":"科研","trait":"高能、结构破坏、天基；高能耗与长准备"},{"id":"lightweight","name":"轻量","trait":"低成本、低维护、分散部署；单体耐久较低"}],"construction":{"elevatorPower":ELEVATOR_POWER_DEMAND},"ai":{"purchaseGrowth":AI_PURCHASE_GROWTH,"upkeepQuadratic":AI_UPKEEP_QUADRATIC,"deployCompute":AI_DEPLOY_COMPUTE},"units":units(),"gpus":gpus(),"rooms":facilities(),"buildings":outdoor(),"plugins":plugins(),"shell":{"costPerCell":SHELL_CELL_COST,"costPerEdge":SHELL_EDGE_COST,"hpExponent":0.85,"minSize":4,"maxSize":24},"research":{"credits":RESEARCH_CREDITS,"compute":RESEARCH_COMPUTE,"scienceData":RESEARCH_DATA,"seconds":RESEARCH_SECONDS,"labComputeUpkeep":LAB_COMPUTE_UPKEEP,"additionalBranchFactor":RESEARCH_ADDITIONAL_BRANCH_FACTOR,"additionalBranchFromTier":RESEARCH_ADDITIONAL_BRANCH_FROM_TIER},"floors":{"aboveground":[1,2,3,4,6],"underground":[0,1,1,2,2]},"victory":{"core":true,"nodeCount":3,"startsAtSeconds":1080,"nodeSeconds":360,"majority":2,"rollbackMultiplier":2,"targetMinutes":[30,45],"forcedTimeout":false}})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    #[test]
    fn approved_roster() {
        for (id, branch, tier) in [
            ("kimi", "speed", 2),
            ("claude", "security", 2),
            ("gpt", "security", 3),
            ("deepseek", "algorithm", 2),
            ("gemini", "science", 2),
            ("minimax", "science", 3),
            ("glm", "lightweight", 2),
        ] {
            let d = unit(id).unwrap();
            assert_eq!(d.branch, branch);
            assert_eq!(d.tier, tier);
            assert_eq!(d.category, "ai");
        }
        for id in ["vscode", "pycharm"] {
            let d = unit(id).unwrap();
            assert_eq!(d.tier, 1);
            assert!(d.branch.is_empty());
            assert_eq!(d.speed, 0.);
        }
        assert_ne!(
            unit("vscode").unwrap().attack_pattern,
            unit("pycharm").unwrap().attack_pattern
        );
    }
    #[test]
    fn each_branch_has_early_counters_and_five_tiers() {
        let u = units();
        for b in BRANCHES {
            for t in 1..=5 {
                assert!(u
                    .iter()
                    .any(|d| d.branch == b && d.tier == t && d.category != "ai"));
            }
            assert!(u
                .iter()
                .any(|d| d.branch == b && d.tier <= 2 && d.target_air));
            assert!(u
                .iter()
                .any(|d| d.branch == b && d.tier <= 2 && d.structure_multiplier >= 1.3));
            assert!(u
                .iter()
                .any(|d| d.branch == b && d.tier <= 2 && d.breach_depth >= 1));
        }
        assert!(facility("airfield").unwrap().branch.is_empty());
    }
    #[test]
    fn unique_ids_and_bounded_ammunition() {
        let u = units();
        let ids: BTreeSet<_> = u.iter().map(|d| &d.id).collect();
        assert_eq!(ids.len(), u.len());
        for d in u {
            assert!(d.cost > 0. && d.hp > 0. && d.damage > 0.);
            if d.ammo_per_shot > 0. {
                let s = d.ammo_capacity / d.ammo_per_shot * d.period;
                assert!((25.0..=45.).contains(&s), "{}:{s}", d.id);
            }
        }
    }
    #[test]
    fn opening_catalog_budget() {
        let shell = 24. * SHELL_CELL_COST + 20. * SHELL_EDGE_COST;
        let fit = 8. * ROOM_CELL_COST
            + facility("data-center").unwrap().cost
            + facility("research-lab").unwrap().cost;
        let cost = shell
            + fit
            + outdoor_building("extractor").unwrap().cost
            + outdoor_building("wind-power").unwrap().cost
            + gpus()[0].cost
            + unit("vscode").unwrap().cost * 2.
            + 90.;
        assert!(STARTING_CREDITS - cost >= 500., "{cost}");
        assert!(
            outdoor_building("wind-power").unwrap().power
                >= gpus()[0].power
                    + facility("data-center").unwrap().power
                    + facility("research-lab").unwrap().power
        );
    }
    #[test]
    fn plugin_branch_category_exclusivity() {
        let p = plugins();
        assert_eq!(p.len(), 15);
        for b in BRANCHES {
            for c in ["core", "attack", "support"] {
                assert_eq!(
                    p.iter()
                        .filter(|d| d.branch == b && d.category == c)
                        .count(),
                    1
                );
            }
        }
    }
    #[test]
    fn ai_budget_and_real_victory() {
        for d in units().into_iter().filter(|d| d.category == "ai") {
            let base = if d.tier == 2 { 260. } else { 520. };
            let factor = if d.branch == "lightweight" { 0.85 } else { 1. };
            assert!(d.cost >= base * factor * 2.8, "{}", d.id);
            assert!(d.damage / d.period < 20. * 1.4_f64.powi(d.tier as i32 - 1) * 3.);
        }
        assert_eq!(catalog()["victory"]["forcedTimeout"], false);
    }
}
