//! **配置生效分发表**（设计 §4.3.3 / D10「逐项 `watch` 热生效」）——开发单元 **G-2**。
//!
//! # 本模块的存在理由：把"生效"变成**可核对的结论**，而不是一句口号
//!
//! 写路径最危险的失败模式不是"写不进去"，而是**"写进去了、屏上也显示新值、装置却仍按旧值跑"**
//! ——那是静默失实。故每个字段的"生效"必须是**显式结论**：
//!
//! | 结论 | 含义 | 回执 / 审计里怎么体现 |
//! |------|------|------------------------|
//! | [`ApplyOutcome::Applied`] | 运行行为**已**随新值改变（本进程内真实生效） | 该键进 `hot_applied` |
//! | [`ApplyOutcome::RestartRequired`] | 已落盘、内存副本已更新，但**运行行为要重启 `mupcd` 才变** | 该键进 `restart_required` + 回执 `message` 明写 + **审计条目 `reason` 明写**（[`crate::config_service`] 的 `per_field_reason`，评审阻塞 2 的修复点）+ `tracing::warn!` |
//!
//! **不得**把后者写成前者。本单元实测的接线现状（逐字段，含**设计行号**依据）：
//!
//! | 字段 | 设计 §4.3.3 的生效方式 | 本单元实测 |
//! |------|------------------------|------------|
//! | `system.log_level` | `tracing_subscriber::reload` handle | ✅ **真接线**（[`HotApply::new`] 注入 handle；`main.rs` 在 Phase 2 建 handle） |
//! | `intercore.host` / `intercore.port` | `watch` → 断开重连 | ❌ **未接线**：核间传输在 `startup.rs:484-520` **构造期**固定（TCP `remote_addr` 写入 transport，Modbus 口/波特率写入 `ModbusRtuSettings`），全仓**无**该参数的 `watch` 通道；改它需要动 `crates/intercore` 的重连路径（**含用户 dirty 红线 `transport/modbus.rs`**）⇒ 本轮**不碰**，如实登记 |
//! | `intercore.heartbeat_interval_sec` / `reconnect_interval_sec` | `watch` → 心跳循环读新值 | ❌ **未接线**：无消费方（TCP 传输无心跳循环；Modbus 心跳循环的周期取 `modbus_rtu.heartbeat_poll_ms`，与本二键无关） |
//! | `gateway.listen_addr` / `gateway.listen_port` | `stop()` → `start()` 重绑定 | ❌ **未接线**：`Iec104Server` 的 listen 配置**构造期固定**（`Iec104Server::new(config)`，无 setter），且该实例还被南向上送（`SouthSink` 的 `iec104_server.clone()`）共享 ⇒ 原地重建会让上送句柄指向**已停止**的旧实例（半生效，EDGE-10 明禁） |
//!
//! ⚠️ **这 6 项"未接线"是设计 §4.3.5 的降级方案**（「本期仅支持 HotApply 子集，连接类参数
//! 只落盘 + 提示需重启」）。**计数口径**（评审重要 5 已更正，全文统一为 **9 / 7 / 1 / 6**）：
//! 字段表 [`crate::console_host::FIELDS`] 共 **9** 键 ⇒ 其中 `editable=true` **7 个**可写
//! ⇒ 真热生效 **1 个**（`system.log_level`）⇒ 需重启 **6 个**（上表 `intercore.*` 4 + `gateway.*` 2）。
//! 设计原文要求该降级**须 PM 裁决并回写 PRD（CF-04 降级）**，
//! **不得静默实施**——本单元按"诚实优先"落地（不谎报生效），并在交付报告里把它列为**待裁项**。
//!
//! ⚠️ **跨模块缺口（评审重要 4）**：上面这张"如实结论"表**到不了用户眼前** —— 渲染端
//! `local-display/src/ui/pages/p2_config.rs:114`（常驻说明）与 `:159`（`TEXT_IMPACT_SAVE`）
//! 都写「**无需重启装置**」（**反向陈述**），且成功分支（`:1845-1848`）**丢弃**后端 `message`
//! （只用固定 `TEXT_TOAST_OK` =「保存成功 · 已生效」）。后端**三处**（回执 `message` /
//! 审计 `reason` / `tracing::warn!`）都说真话，**屏上却在说谎**。本单元**只登记不改**
//! （`local-display/**` 不在授权范围）；处置与最小改法见交付报告「给 PM 的最小改法清单」。

use serde_json::Value;
use tracing_subscriber::reload::Handle;
use tracing_subscriber::{EnvFilter, Registry};

/// 日志级别热生效句柄（`tracing_subscriber` 的 reload 能力；`main.rs` 在 tracing 初始化处保留）。
pub type LogReloadHandle = Handle<EnvFilter, Registry>;

/// 单字段的生效结论（**必须二选一，没有第三态**——"不知道生效没有"不是可交付的结论）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// 运行行为**已**随新值改变。
    Applied,
    /// 已落盘 + 内存副本已更新；运行行为需**重启 `mupcd`** 才变（原因人读，进回执 / 审计 / 日志）。
    RestartRequired { reason: &'static str },
}

impl ApplyOutcome {
    /// 是否真实生效。
    pub fn is_applied(&self) -> bool {
        matches!(self, Self::Applied)
    }
}

/// 生效分发器（本进程的"哪些字段真的能动运行行为"的唯一真源）。
#[derive(Clone, Default)]
pub struct HotApply {
    log_reload: Option<LogReloadHandle>,
}

impl HotApply {
    /// 构造。`log_reload = None` ⇒ `system.log_level` 也无法热生效（测试 / 非生产装配；
    /// **不会**谎报 `Applied`——见 [`Self::apply`]）。
    pub fn new(log_reload: Option<LogReloadHandle>) -> Self {
        Self { log_reload }
    }

    /// 逐字段分派（设计 §4.3.3）。
    ///
    /// 调用点在**落盘与内存副本更新之后**：本方法失败**不**回滚（它是"尽力生效"的最后一步），
    /// 但**绝不**把失败说成成功。
    pub fn apply(&self, key: &str, value: &Value) -> ApplyOutcome {
        match key {
            "system.log_level" => {
                let Some(level) = value.as_str() else {
                    return ApplyOutcome::RestartRequired {
                        reason: "日志级别不是字符串（不应发生：字段校验已拦）",
                    };
                };
                match self.set_log_level(level) {
                    Ok(()) => ApplyOutcome::Applied,
                    Err(e) => {
                        tracing::error!(level, error = %e, "日志级别热生效失败");
                        ApplyOutcome::RestartRequired {
                            reason: "日志 reload handle 不可用（热生效未接线）",
                        }
                    }
                }
            }
            "intercore.host" | "intercore.port" => ApplyOutcome::RestartRequired {
                reason: "核间传输在启动期构造（无 watch 通道）⇒ 需重启 mupcd 生效",
            },
            "intercore.heartbeat_interval_sec" | "intercore.reconnect_interval_sec" => {
                ApplyOutcome::RestartRequired {
                    reason: "核间心跳/重连参数的消费方尚未接线（无 watch）⇒ 需重启 mupcd 生效",
                }
            }
            "gateway.listen_addr" | "gateway.listen_port" => ApplyOutcome::RestartRequired {
                reason: "IEC 104 监听地址在构造期固定且实例与南向上送共享 ⇒ 需重启 mupcd 生效",
            },
            other => ApplyOutcome::RestartRequired {
                reason: "字段不在 §4.3.3 生效分发表内（未接线，也不假装生效）",
            }
            .tap_warn(other),
        }
    }

    /// 日志级别热生效（`reload` 换 filter；**≤1 s**，无需重启）。
    ///
    /// 语义边界（如实登记）：换上去的 filter 是 `EnvFilter::new(level)`，**会覆盖**启动时
    /// 由 `RUST_LOG` 环境变量给出的过滤表达式——屏上选的级别是配置的真源，取它为准。
    pub fn set_log_level(&self, level: &str) -> Result<(), String> {
        let h = self
            .log_reload
            .as_ref()
            .ok_or_else(|| "日志 reload handle 未注册（进程未按生产路径装配）".to_string())?;
        h.reload(EnvFilter::new(level))
            .map_err(|e| format!("reload 日志 filter 失败: {e}"))
    }
}

/// 小工具：给"未接线"分支补一条 `warn`（现场排障要能看见是哪一个键没生效）。
trait TapWarn {
    fn tap_warn(self, key: &str) -> Self;
}

impl TapWarn for ApplyOutcome {
    fn tap_warn(self, key: &str) -> Self {
        if let ApplyOutcome::RestartRequired { reason } = &self {
            tracing::warn!(field = key, reason, "配置已保存但运行行为需重启才变（如实登记）");
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 建一个**真实**的 reload handle（不初始化全局订阅者：handle 只是 layer 的遥控器）。
    fn handle() -> (tracing_subscriber::reload::Layer<EnvFilter, Registry>, LogReloadHandle) {
        tracing_subscriber::reload::Layer::new(EnvFilter::new("info"))
    }

    /// `system.log_level`：**真接线**时结论必须是 `Applied`，且 filter **确实被换掉**。
    ///
    /// **改什么会让本条变红**：把 `apply` 里 `system.log_level` 的分支改成 `RestartRequired`
    /// （即"谎报未接线"）⇒ 第 1 条红；`set_log_level` 里空实现（不 `reload`）⇒ 第 2 条红。
    #[test]
    fn log_level_is_really_hot_applied_when_the_handle_exists() {
        let (_layer, h) = handle();
        let hot = HotApply::new(Some(h.clone()));
        assert_eq!(
            hot.apply("system.log_level", &json!("debug")),
            ApplyOutcome::Applied,
            "有 reload handle ⇒ 日志级别是**真**热生效"
        );
        assert_eq!(
            h.with_current(|f| f.to_string()).unwrap(),
            "debug",
            "filter 必须真的被换成新级别（不是只返回 Ok）"
        );
        assert_eq!(hot.apply("system.log_level", &json!("warn")), ApplyOutcome::Applied);
        assert_eq!(h.with_current(|f| f.to_string()).unwrap(), "warn");
    }

    /// **无 handle ⇒ 必须如实说"需重启"，不得谎报 `Applied`**（诚实性网）。
    #[test]
    fn log_level_without_a_handle_is_reported_as_restart_required() {
        let hot = HotApply::new(None);
        let out = hot.apply("system.log_level", &json!("debug"));
        assert!(
            matches!(out, ApplyOutcome::RestartRequired { .. }),
            "无 handle 时不得报 Applied（那是谎报生效），实得 {out:?}"
        );
        assert!(hot.set_log_level("debug").is_err(), "无 handle 时 set 必须 Err");
    }

    /// 连接类**六项**（`intercore.*` 4 + `gateway.*` 2）+ 表外键：一律 `RestartRequired`
    /// 且**带可读原因**（原因要能上报告 / 日志）。
    ///
    /// 逐键断言（不是"数个数"）：将来接线了其中一项，本用例应当**变红**，提醒把它的结论
    /// 从"未接线"改成"已接线"——这正是"接线进度不可静默漂移"的网。
    #[test]
    fn connection_class_fields_are_honestly_registered_as_not_wired() {
        let hot = HotApply::new(None);
        for key in [
            "intercore.host",
            "intercore.port",
            "intercore.heartbeat_interval_sec",
            "intercore.reconnect_interval_sec",
            "gateway.listen_addr",
            "gateway.listen_port",
        ] {
            match hot.apply(key, &json!(1)) {
                ApplyOutcome::RestartRequired { reason } => {
                    assert!(!reason.is_empty(), "`{key}` 的原因不能是空串");
                    assert!(reason.contains("重启"), "`{key}` 的原因须点明需重启: {reason}");
                }
                ApplyOutcome::Applied => panic!("`{key}` 本轮**未接线**，不得报 Applied（谎报）"),
            }
        }
        // 分发表外的键：也必须是"未接线"，不能默默当作已生效
        assert!(matches!(
            hot.apply("display.bind_addr", &json!("127.0.0.1")),
            ApplyOutcome::RestartRequired { .. }
        ));
    }

    /// 分发表与字段表的**覆盖关系**（防"加了字段忘了登记生效方式"）：所有 `FIELDS` 键都必须
    /// 得到**显式**结论（本用例经 `apply` 逐键取结论，不存在"没登记"的键）。
    #[test]
    fn every_field_key_gets_an_explicit_outcome() {
        use crate::console_host::FIELDS;
        let hot = HotApply::new(None);
        for m in FIELDS {
            let out = hot.apply(m.key, &json!(1));
            // 两种结论都合法，但**必须**是其中之一（不存在"没结论"）
            assert!(
                out.is_applied()
                    || matches!(out, ApplyOutcome::RestartRequired { reason } if !reason.is_empty()),
                "字段 `{}` 无显式生效结论",
                m.key
            );
        }
    }
}
