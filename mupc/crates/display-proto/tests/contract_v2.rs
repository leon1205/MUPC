//! 契约 v2 集成断言（**只用公开 API + 字面量 JSON**）。
//!
//! 为什么单独一层：模块内单测在 crate 内部，与实现同源——若实现与设计同错，往返断言
//! （`to_string` → `from_str`）**照样通过**，掩盖契约偏差。本文件以**手写字面量 JSON**
//! 钉死线上线格式（字段名 / 枚举词表 / `null` 语义），任何字段改名或语义漂移都会在此失败。
//!
//! 覆盖（设计 §11.1 `display-proto` 行）：
//! 帧 v2 往返（含新段缺省）、旧帧兼容（v1 帧 → v2 类型）、未知字段容忍、
//! `PROTO_VERSION` 不匹配拒绝、畸形帧（超限）拒绝、控制信封 / 回执 / 错误码、
//! 幂等与审计 fail-closed、配置字段越界拒绝。

use mupc_display_proto::*;

/// 设计 §3.1 的完整 v2 帧（字面量；四新段齐备）。
const V2_FRAME_JSON: &str = r#"{
    "version": 2,
    "seq": 123,
    "ts_ms": 1757412000000,
    "soc": 65.0,
    "soc_source": "pcs_reg1010",
    "soc_flag": "valid",
    "run_state": 2,
    "pcs_online": true,
    "p_phase": [ {"v":12.3,"flag":"valid"}, {"v":11.8,"flag":"valid"}, {"v":12.0,"flag":"valid"} ],
    "p_total": {"v":36.1,"flag":"valid"},
    "i_phase": [ {"v":22.5,"flag":"valid"}, {"v":22.1,"flag":"valid"}, {"v":22.3,"flag":"valid"} ],
    "inconsistency": false,
    "device": {
        "ts_ms": 1757412000000,
        "uptime_secs": 3600,
        "cpu_temp_c": 48.5,
        "mem_used_pct": 31.2,
        "iec104": "connected",
        "intercore": "connecting",
        "hmi_channel": "unknown",
        "control_source": "ai_disabled"
    },
    "alarms": {
        "ts_ms": 1757412000000,
        "available": false,
        "items": []
    },
    "info": {
        "firmware_version": "0.1.0",
        "build_time": null,
        "model": null,
        "serial": null,
        "service_scope": "loopback_only",
        "mgmt_ipv4": null
    },
    "interlock": {
        "ts_ms": 1757412000000,
        "available": false,
        "enabled": false,
        "latched": false,
        "stop_failed": false,
        "sources": [],
        "fault_lamp": null,
        "run_lamp": null,
        "release_hold_secs": 0
    }
}"#;

#[test]
fn v2_frame_wire_shape_is_pinned() {
    let frame = DisplayFrame::from_json_slice(V2_FRAME_JSON.as_bytes()).expect("合法 v2 帧须可解码");

    // v1 段语义不变
    assert_eq!(frame.version, 2);
    assert_eq!(frame.seq, 123);
    assert_eq!(frame.ts_ms, 1_757_412_000_000);
    assert_eq!(frame.soc, Some(65.0));
    assert_eq!(frame.soc_source, SocSource::PcsReg1010);
    assert_eq!(frame.run_state, Some(RunState::Charge));
    assert!(frame.pcs_online && !frame.inconsistency);
    assert_eq!(frame.p_total.v, Some(36.1));

    // v2 新段
    assert_eq!(frame.device.uptime_secs, Some(3600));
    assert_eq!(frame.device.mem_used_pct, Some(31.2));
    assert_eq!(frame.device.iec104, LinkState::Connected);
    assert_eq!(frame.device.intercore, LinkState::Connecting);
    assert_eq!(frame.device.hmi_channel, LinkState::Unknown);
    assert_eq!(frame.device.control_source, ControlSource::AiDisabled);
    assert!(!frame.alarms.available, "available=false ≠ 无告警（EDGE-09）");
    assert_eq!(frame.info.firmware_version, "0.1.0");
    assert_eq!(frame.info.build_time, None, "null → None → 「未提供」（EDGE-16）");
    assert_eq!(frame.info.service_scope, ServiceScope::LoopbackOnly);
    assert_eq!(frame.info.mgmt_ipv4, None);
    assert!(!frame.interlock.available, "available=false ≠ 未联锁（IL-01.6）");
    assert_eq!(frame.interlock.fault_lamp, None, "null → 未知，不臆造为灭");

    // 再编码：字段名与 null 语义必须逐字保持（防实现侧改名 / 补 0）
    let out = serde_json::to_string(&frame).unwrap();
    for needle in [
        "\"version\":2",
        "\"soc_source\":\"pcs_reg1010\"",
        "\"run_state\":2",
        "\"p_phase\":[",
        "\"device\":{",
        "\"iec104\":\"connected\"",
        "\"control_source\":\"ai_disabled\"",
        "\"alarms\":{",
        "\"info\":{",
        "\"service_scope\":\"loopback_only\"",
        "\"build_time\":null",
        "\"mgmt_ipv4\":null",
        "\"interlock\":{",
        "\"fault_lamp\":null",
    ] {
        assert!(out.contains(needle), "编码结果缺 `{needle}`：{out}");
    }
    // 严禁把 None 补成 0 / 假值
    assert!(!out.contains("\"soc\":0"), "禁补 0");
    assert!(!out.contains("\"build_time\":\"\""));

    // 稳定往返
    let back: DisplayFrame = serde_json::from_str(&out).unwrap();
    assert_eq!(back, frame);
}

#[test]
fn v1_frame_into_v2_type_degrades_explicitly_and_version_is_rejected() {
    // 真实 v1 发布方：无四新段，version=1
    let v1 = r#"{"version":1,"seq":9,"ts_ms":1757412000000,"soc":null,
        "soc_source":"lost","soc_flag":"offline","run_state":null,"pcs_online":false,
        "p_phase":[{"v":null,"flag":"offline"},{"v":null,"flag":"offline"},{"v":null,"flag":"offline"}],
        "p_total":{"v":null,"flag":"offline"},
        "i_phase":[{"v":null,"flag":"offline"},{"v":null,"flag":"offline"},{"v":null,"flag":"offline"}],
        "inconsistency":false,"some_future_key":42}"#;

    // 1) 类型层可容忍（段取 Default）；未知字段容忍
    let frame: DisplayFrame = serde_json::from_str(v1).unwrap();
    assert_eq!(frame.soc, None);
    assert_eq!(frame.soc_source, SocSource::Lost);
    assert_eq!(frame.run_state, None);
    assert!(!frame.alarms.available && !frame.interlock.available);
    assert_eq!(frame.device.iec104, LinkState::Unknown);
    assert_eq!(frame.device.control_source, ControlSource::Unknown);
    assert_eq!(frame.info.firmware_version, "");

    // 2) 但版本校验**必须拒绝**——绝不允许静默按 v2 语义展示 v1 帧
    let err = DisplayFrame::from_json_slice(v1.as_bytes()).unwrap_err();
    assert!(
        matches!(err, Error::ProtoVersionMismatch { got: 1, expected: 2 }),
        "v1 帧必须被拒绝，实际: {err:?}"
    );
    assert!(err.to_string().contains("version mismatch"));

    // 3) 未来版本同样拒绝（不静默接受）
    let v3 = V2_FRAME_JSON.replace("\"version\": 2", "\"version\": 3");
    assert!(matches!(
        DisplayFrame::from_json_slice(v3.as_bytes()),
        Err(Error::ProtoVersionMismatch { got: 3, expected: 2 })
    ));
}

#[test]
fn malformed_frame_is_rejected_not_parsed() {
    // 超限：先于 JSON 解析拒绝
    let huge = vec![b'x'; MAX_FRAME_BYTES + 16];
    assert!(matches!(
        DisplayFrame::from_json_slice(&huge),
        Err(Error::FrameTooLarge { .. })
    ));
    // 畸形 JSON：Err(Json)，不 panic
    assert!(matches!(
        DisplayFrame::from_json_slice(b"{\"version\": 2, "),
        Err(Error::Json(_))
    ));
    // 枚举外运行态：反序列化即拒（越界不得被接受）
    let bad = V2_FRAME_JSON.replace("\"run_state\": 2", "\"run_state\": 7");
    assert!(DisplayFrame::from_json_slice(bad.as_bytes()).is_err());
}

#[test]
fn control_envelope_write_path_is_pinned() {
    // 写请求字面量（配置文件保存）
    let req_json = r#"{
        "request_id": "0f7a3e10-1111-4222-8333-444455556666",
        "issued_at_ms": 1757412000000,
        "op": "apply",
        "payload": {"changes": {"intercore.port": 502}, "from": "edit"}
    }"#;
    let req: ControlRequest<ConfigPatch> = serde_json::from_str(req_json).unwrap();
    assert_eq!(req.op, "apply");
    assert_eq!(req.payload.from, PatchSource::Edit);
    assert_eq!(req.payload.changes["intercore.port"], serde_json::json!(502));
    // 信封 + 路由校验通过
    assert!(req
        .validate_for(ConsoleEndpoint::ConfigApply, 1_757_412_000_000)
        .is_ok());
    // 幂等键 = (op, request_id)
    assert_eq!(
        req.idempotency_key(),
        IdempotencyKey::new("apply", "0f7a3e10-1111-4222-8333-444455556666")
    );

    // 失败回执字面量：字段级原因必须可被 UI 逐字段读取
    let resp_json = r#"{
        "request_id": "0f7a3e10-1111-4222-8333-444455556666",
        "ok": false,
        "code": "rejected_validation",
        "message": "1 个字段校验失败",
        "applied": null,
        "field_errors": [{"field":"intercore.port","reason":"越界，允许区间 [1, 65535]"}],
        "audit_id": "audit-0001",
        "duplicate": false,
        "at_ms": 1757412000100
    }"#;
    let resp: ControlResponse<ConfigView> = serde_json::from_str(resp_json).unwrap();
    assert!(!resp.ok && resp.code.is_rejection());
    assert_eq!(resp.applied, None);
    assert_eq!(resp.field_errors[0].field, "intercore.port");
    assert_eq!(resp.audit_id.as_deref(), Some("audit-0001"));
}

#[test]
fn idempotency_and_audit_fail_closed_are_enforced_by_contract() {
    // 幂等命中：回首次结果，仅 duplicate 置位
    let mut first: ControlResponse<InterlockOpAck> = ControlResponse::ok(
        "rid-1",
        Some(InterlockOpAck {
            latched: false,
            stopped: true,
        }),
        Some("audit-1".into()),
        1_000,
    );
    assert!(first.ok && !first.duplicate);
    let mut replay = first.clone();
    replay.mark_duplicate();
    assert_eq!(replay.applied, first.applied, "重复请求回首次结果");
    assert!(replay.duplicate && replay.audit_id == first.audit_id);
    first.ok = false;
    first.code = ControlCode::RejectedPrecondition;
    let mut replay_reject = first.clone();
    replay_reject.mark_duplicate();
    assert!(!replay_reject.ok, "首次被拒 → 重放同样被拒（不得因重放变成功）");

    // 审计不可写 → fail-closed：未执行、无生效值、错误码钦定
    let fail_closed: bool = AUDIT_FAIL_CLOSED;
    assert!(fail_closed, "契约默认裁决为 fail-closed");
    let resp: ControlResponse<ConfigView> = ControlResponse::audit_unavailable("rid-2", 2_000);
    assert!(!resp.ok);
    assert_eq!(resp.code, ControlCode::AuditUnavailable);
    assert_eq!(resp.applied, None, "审计失败必须不落盘 / 不生效");
    assert_eq!(resp.at_ms, 2_000);
}

#[test]
fn config_field_out_of_range_is_rejected_with_specific_reason() {
    let field = ConfigField {
        key: "intercore.port".into(),
        label: "核间本地端口".into(),
        kind: ConfigKind::U16 {
            min: 1,
            max: 65535,
            step: 1,
        },
        value: serde_json::json!(502),
        default: serde_json::json!(502),
        unit: None,
        requires_reconnect: true,
        editable: true,
    };
    assert!(field.validate_value(&serde_json::json!(502)).is_ok());
    // 越界（PRD 6.5：不得静默接受）——区间外
    let reason = field.validate_value(&serde_json::json!(0)).unwrap_err();
    assert!(reason.contains("越界"), "须给出具体原因: {reason}");
    // 超出 u16 表示范围亦须拒绝且原因具体（不得截断 / 回绕后接受）
    let reason = field.validate_value(&serde_json::json!(70000)).unwrap_err();
    assert!(reason.contains("u16 上限"), "须给出具体原因: {reason}");
    // 类型非法
    assert!(field
        .validate_value(&serde_json::json!("not-a-number"))
        .is_err());

    // 只读字段（回环安全红线 PL-4）不可经屏修改
    let mut ro = field.clone();
    ro.key = "display.bind_addr".into();
    ro.editable = false;
    ro.kind = ConfigKind::Ipv4;
    let reason = ro.validate_value(&serde_json::json!("0.0.0.0")).unwrap_err();
    assert!(reason.contains("只读"), "只读字段须被后端二次校验拒绝: {reason}");
}
