//! 安全 / 联锁（F16–F18）契约——设计 §4.6。
//!
//! 迁移口径：原 `web-api::app_state::{InterlockStatus, InterlockSourceStatus}` 迁至本模块，
//! 并把 `Result<(), String>` 的**字符串错误**改造为结构化 [`InterlockReject`]——EDGE-12 /
//! IL-02 / IL-03 要求「**明示具体拒绝原因**，不得静默失败」，字符串只够打日志、不够驱动 UI。
//!
//! 语义名优先：`fault_lamp` / `run_lamp` **不绑 DO 号**（PRD F16 与既有代码注释的 DO1/DO2
//! 归属相反，见设计 §4.6 备注 / §14 R-09）；DO 号映射由部署侧 `io.do` 配置表决定。

use serde::{Deserialize, Serialize};

pub use crate::frame::{InterlockSection, InterlockSourceItem};

/// 联锁状态视图（F16）——设计 §4.6 `InterlockApi::status()` 的返回类型。
///
/// 即帧内 `interlock` 段（设计 §3.1）的同一形态：**契约单一真源**，
/// 避免「trait 返回一套、帧里又是另一套」的双源漂移。
///
/// 含 `available` / `enabled`：`available=false` → 显「联锁状态不可用」，
/// **不得**显「未联锁」（IL-01.6，二者语义不同）。
pub type InterlockView = InterlockSection;

/// 联锁状态（F16）——迁移名（原 `web-api::app_state::InterlockStatus`）。
///
/// 保留为 [`InterlockView`] 的别名，供 `mupc-core-bin/src/interlock.rs` 与
/// 既有调用点平滑 `use` 迁移（设计 §4.6 迁移表；改名不改语义）。
pub type InterlockStatus = InterlockView;

/// 单个触发源状态（F16）——即 [`InterlockSourceItem`] 的迁移别名。
pub type InterlockSourceStatus = InterlockSourceItem;

/// 联锁写操作的结构化拒绝原因（设计 §4.6；EDGE-12：**不得静默失败**）。
///
/// 每个变体都必须有中文用户可读文案（[`InterlockReject::user_message`]），
/// UI 在弹层内就地展示（设计 §11.3：每个变体须有对应用例）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum InterlockReject {
    /// 触发源未复位（列出具体源）。
    #[error("触发源未复位：{}", remaining.join(", "))]
    SourcesNotReset {
        /// 仍未复位的触发源名（如 `estop` / `door`）。
        remaining: Vec<String>,
    },
    /// 保持时间不足（须保持 `need_secs`，尚余 `remaining_secs`）。
    #[error("保持时间不足，还需 {remaining_secs} s（须保持 {need_secs} s）")]
    HoldNotElapsed {
        /// 释放前置条件要求的保持秒数。
        need_secs: u64,
        /// 尚余秒数。
        remaining_secs: u64,
    },
    /// 处于 latch 态（`ack_m1` 前置条件不满足）。
    #[error("处于 latch 态，须先释放联锁")]
    Latched,
    /// 停机未确认（`stop_failed`）。
    #[error("PCS 停机未确认，暂不可授权重启")]
    StopPending,
    /// 联锁功能未启用（`io.enabled=false`）。
    #[error("联锁功能未启用")]
    NotEnabled,
    /// 上一操作仍在处理中。
    #[error("上一操作正在处理中，请稍候")]
    Busy,
    /// 内部错误（透传具体原因，不吞）。
    #[error("内部错误：{0}")]
    Internal(String),
}

impl InterlockReject {
    /// 中文用户可读文案（UI 直接展示；设计 §4.6）。
    ///
    /// 与 `Display` 一致，但显式提供以便 UI 侧不依赖 `to_string()` 的格式化间接层。
    pub fn user_message(&self) -> String {
        self.to_string()
    }
}

/// 联锁后端接口（设计 §4.6，**逐字对齐**）。
///
/// 迁移口径：原 `web-api::app_state::InterlockApi` 迁至本模块（HMI 契约单一真源）；
/// 错误类型由 `Result<(), String>` 结构化改造为 [`InterlockReject`]——EDGE-12 / IL-02 /
/// IL-03 要求「明示具体拒绝原因」，字符串只够打日志、不够驱动 UI。
///
/// 真实实现由 mupcd 侧 `InterlockController` 提供（工作单元 J/K；本契约层**不含**实现）。
/// `async-trait` 为在 `dyn` 擦除（`Arc<dyn InterlockApi>`）下承载 async 方法所必需，
/// 属过程宏依赖、非 UI 依赖（PM 裁定：契约层零 UI 依赖约束不排斥它）。
#[async_trait::async_trait]
pub trait InterlockApi: Send + Sync {
    /// 查询联锁总体状态（含 `available` / `enabled`）。
    async fn status(&self) -> InterlockView;

    /// 人工释放联锁：触发源已复位且保持 ≥ `release_hold_secs` 才清 latch。
    async fn request_release(&self) -> std::result::Result<(), InterlockReject>;

    /// M1 保护跳闸 / 停机人工授权重启（仅 `!latched` 生效）。
    async fn ack_m1(&self) -> std::result::Result<(), InterlockReject>;
}

/// 联锁写操作负载（`POST .../interlock/release` 与 `.../ack_m1`；设计 §3.4）。
///
/// `observed_*` 为 UI 提交时**它看到的**联锁态：后端与服务端当前态比对，若已变化 →
/// [`crate::control::ControlCode::RejectedPrecondition`] +「联锁状态已变化，请刷新后重试」
/// （无并发控制场景下的乐观并发检查，防「基于过期画面执行破坏性操作」，EDGE-19）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterlockOpPayload {
    /// UI 观测到的 latch 态。
    pub observed_latched: bool,
    /// UI 观测到的触发源名列表（与服务端比对）。
    pub observed_sources: Vec<String>,
}

/// 联锁写操作回执（成功时作为控制回执的 `applied`，供 UI **立即**刷新不等下一帧；
/// 设计 §3.4 / §6.4 F17.6）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterlockOpAck {
    /// 操作后 latch 态。
    pub latched: bool,
    /// 操作后停机确认态。
    pub stopped: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 每个变体都须有明确中文文案（设计 §11.3：EDGE-12「不得静默失败」的直接落点）----
    #[test]
    fn every_reject_variant_has_specific_user_message() {
        let cases: Vec<(InterlockReject, &[&str])> = vec![
            (
                InterlockReject::SourcesNotReset {
                    remaining: vec!["estop".into(), "door".into()],
                },
                &["触发源未复位", "estop", "door"],
            ),
            (
                InterlockReject::HoldNotElapsed {
                    need_secs: 30,
                    remaining_secs: 12,
                },
                &["保持时间不足", "12", "30"],
            ),
            (InterlockReject::Latched, &["latch 态", "先释放联锁"]),
            (InterlockReject::StopPending, &["停机未确认"]),
            (InterlockReject::NotEnabled, &["未启用"]),
            (InterlockReject::Busy, &["处理中"]),
            (InterlockReject::Internal("io 句柄丢失".into()), &["内部错误", "io 句柄丢失"]),
        ];
        assert_eq!(cases.len(), 7, "全部 7 个变体均须有对应用例");
        for (reject, needles) in cases {
            let msg = reject.user_message();
            assert!(!msg.is_empty());
            for n in needles {
                assert!(msg.contains(n), "{reject:?} 文案缺 `{n}`，实际: {msg}");
            }
        }
    }

    #[test]
    fn reject_json_is_structured_not_stringly() {
        // 结构化拒绝原因可被 UI 解析出具体字段（而非只有一个字符串）
        let json = r#"{"sources_not_reset":{"remaining":["estop","door"]}}"#;
        let r: InterlockReject = serde_json::from_str(json).unwrap();
        assert_eq!(
            r,
            InterlockReject::SourcesNotReset {
                remaining: vec!["estop".into(), "door".into()]
            }
        );
        let h = InterlockReject::HoldNotElapsed { need_secs: 30, remaining_secs: 12 };
        assert_eq!(
            serde_json::to_string(&h).unwrap(),
            r#"{"hold_not_elapsed":{"need_secs":30,"remaining_secs":12}}"#
        );
    }

    #[test]
    fn op_payload_and_ack_literal_json() {
        let p: InterlockOpPayload = serde_json::from_str(
            r#"{"observed_latched":false,"observed_sources":["estop"]}"#,
        )
        .unwrap();
        assert!(!p.observed_latched);
        assert_eq!(p.observed_sources, vec!["estop".to_string()]);
        let a: InterlockOpAck = serde_json::from_str(r#"{"latched":false,"stopped":true}"#).unwrap();
        assert!(!a.latched && a.stopped);
    }

    /// 迁移别名：视图即帧内联锁段——「不可用」与「未联锁」不得混淆（IL-01.6）。
    #[test]
    fn status_alias_default_is_unavailable_not_unlatched() {
        let s = InterlockStatus::default();
        assert!(!s.available, "缺省 = 状态不可用");
        assert!(!s.latched);
        assert!(!s.enabled);
        assert!(s.sources.is_empty());
        assert_eq!(s.fault_lamp, None, "灯态未知显「未知」，不臆造为灭");
        let item = InterlockSourceStatus {
            name: "estop".into(),
            tripped: true,
        };
        assert!(item.tripped);
    }

    /// `InterlockView` / `InterlockStatus` 与帧内联锁段是**同一类型**（单一真源，防双源漂移）。
    #[test]
    fn view_status_and_frame_section_are_same_type() {
        let v: InterlockView = InterlockSection {
            available: true,
            enabled: true,
            ..Default::default()
        };
        let s: InterlockStatus = v.clone();
        assert_eq!(v, s, "视图与迁移名必须同型同值");
        assert!(s.available && s.enabled, "View 必含 available/enabled（设计 §4.6）");
        // 经 serde 往返后仍是同一形态（契约跨进程一致性）
        let back: InterlockView = serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(back, v);
    }

    // ── 以下为 trait 形状测试：无 tokio 依赖，用最小 no-op 执行器轮询（测试桩 futures 不 await）──

    /// 最小 `block_on`：仅用于测试桩中**无 await 点**的 future（poll 一次即 Ready）。
    fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        use std::sync::Arc;
        use std::task::{Context, Poll, Wake, Waker};
        struct NoopWake;
        impl Wake for NoopWake {
            fn wake(self: Arc<Self>) {}
        }
        let waker = Waker::from(Arc::new(NoopWake));
        let mut cx = Context::from_waker(&waker);
        let mut fut = Box::pin(fut);
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("测试桩 future 不得返回 Pending（无 await 点）"),
        }
    }

    /// 测试桩实现，用于钉死 trait 的可实现性与三个方法签名。
    struct MockInterlock {
        view: InterlockView,
        reject: Option<InterlockReject>,
    }

    #[async_trait::async_trait]
    impl InterlockApi for MockInterlock {
        async fn status(&self) -> InterlockView {
            self.view.clone()
        }
        async fn request_release(&self) -> std::result::Result<(), InterlockReject> {
            match &self.reject {
                Some(r) => Err(r.clone()),
                None => Ok(()),
            }
        }
        async fn ack_m1(&self) -> std::result::Result<(), InterlockReject> {
            match &self.reject {
                Some(r) => Err(r.clone()),
                None => Ok(()),
            }
        }
    }

    #[test]
    fn interlock_api_trait_is_dyn_object_safe_and_signature_pinned() {
        let api: std::sync::Arc<dyn InterlockApi> = std::sync::Arc::new(MockInterlock {
            view: InterlockView {
                available: true,
                enabled: true,
                latched: false,
                sources: vec![InterlockSourceStatus {
                    name: "estop".into(),
                    tripped: false,
                }],
                ..Default::default()
            },
            reject: None,
        });
        // 1) 类型擦除可用（`Arc<dyn InterlockApi>`，与 mupcd 装配路径一致）
        let v = block_on(api.status());
        assert!(v.available && v.enabled && !v.latched);
        assert_eq!(v.sources[0].name, "estop");
        // 2) 正常路径 Ok
        assert!(block_on(api.request_release()).is_ok());
        assert!(block_on(api.ack_m1()).is_ok());

        // 3) 拒绝路径：结构化原因可被 UI 逐字段读取（而非单个字符串）
        let rejecting: std::sync::Arc<dyn InterlockApi> = std::sync::Arc::new(MockInterlock {
            view: InterlockView::default(),
            reject: Some(InterlockReject::HoldNotElapsed {
                need_secs: 30,
                remaining_secs: 7,
            }),
        });
        let e = block_on(rejecting.request_release()).unwrap_err();
        assert_eq!(
            e,
            InterlockReject::HoldNotElapsed {
                need_secs: 30,
                remaining_secs: 7
            }
        );
        assert!(e.user_message().contains("还需 7 s"));
        assert!(block_on(rejecting.ack_m1()).is_err());
    }
}
