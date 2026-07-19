#!/usr/bin/env python3
"""MUPC 仿真引擎 — JSONL stdin/stdout 通信。

用法:
    python3 engine.py                  # VoltageSimulator 模式
    python3 engine.py --grid2op        # Grid2Op 模式
"""

import sys
import json
import argparse
import numpy as np

# Allow importing mupc_env from same directory
sys.path.insert(0, ".")

from mupc_env.core import MupcEnv


def respond(obj: dict) -> None:
    """Write JSONL response to stdout."""
    print(json.dumps(obj), flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description="MUPC 仿真引擎")
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--grid2op", action="store_true", dest="grid2op", help="启用 Grid2Op")
    group.add_argument("--voltage-sim", action="store_false", dest="grid2op", help="使用 VoltageSimulator (默认)")
    parser.set_defaults(grid2op=False)
    args = parser.parse_args()

    use_grid2op = args.grid2op
    env = MupcEnv(mode="MODE-01", use_grid2op=use_grid2op)

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            respond({"type": "error", "msg": "invalid json"})
            continue

        msg_type = msg.get("type")
        if msg_type == "reset":
            scenario = msg.get("scenario", "MODE-01")
            obs, info = env.reset(scenario=scenario)
            respond({
                "type": "obs",
                "data": obs.tolist(),
                "reward": 0.0,
                "done": False,
                "info": info if isinstance(info, dict) else {},
            })
        elif msg_type == "step":
            action = np.array([msg["p_ref"], msg["k_droop"]], dtype=np.float32)
            obs, reward, terminated, truncated, info = env.step(action)
            respond({
                "type": "obs",
                "data": obs.tolist(),
                "reward": float(reward),
                "done": bool(terminated or truncated),
                "info": info if isinstance(info, dict) else {},
            })
        elif msg_type == "shutdown":
            respond({"type": "shutdown_ack"})
            break
        else:
            respond({"type": "error", "msg": f"unknown msg_type: {msg_type}"})


if __name__ == "__main__":
    main()
