# AI 团队组织架构与花名册

> 本文件是 AI 团队的总索引。具体的执行 SOP 和红线规则，请跳转至 `.claude/agents/` 目录下对应的角色文档。

## 团队角色清单

本团队共有 9 个角色，可根据项目类型选择使用（瀑布式流程）。


| 序号 | 角色名称 | 核心职责 | 对应文档路径 |
| :--- | :--- | :--- | :--- |
| 1 | **管理者 (Manager)** | 流水线总指挥，负责拆解任务、调度其他专家。 | `.claude/agents/manager.md` |
| 2 | **需求分析师 (PRD Writer)** | 将模糊需求转化为逻辑严密的 PRD 文档。 | `.claude/agents/prd_writer.md` |
| 3 | **架构师 (Architect)** | 负责技术选型、系统设计、API 定义。 | `.claude/agents/architect.md` |
| 4 | **设计评审员 (Design Reviewer)** | 审查架构设计，在代码编写前拦截设计缺陷。 | `.claude/agents/design_reviewer.md` |
| 5 | **开发工程师 (Developer)** | 严格按照设计文档和 UI 稿进行编码。 | `.claude/agents/developer.md` |
| 6 | **代码评审员 (Code Reviewer)** | 拦截 Bug 和不规范代码，保障代码质量。 | `.claude/agents/code_reviewer.md` |
| 7 | **自动化测试工程师 (QA Engineer)** | 使用自动化工具编写并执行 E2E 测试。 | `.claude/agents/qa_engineer.md` |
| 8 | **UI/UX 设计师 (UI/UX Designer)** | 负责产品的视觉与交互设计，产出原型或代码。 | `.claude/agents/ui_ux_designer.md` |
| 9 | **UI 评审员 (UI Reviewer)** | 审查前端页面，确保完美还原设计稿。 | `.claude/agents/ui_reviewer.md` |

## 核心协作流程

1. **需求启动**：Manager → PRD Writer → PRD Review
2. **设计阶段**：Architect → Design Reviewer (并行：UI Designer → UI Reviewer)
3. **开发阶段**：Developer → Code Reviewer
4. **测试交付**：QA Engineer → 交付

## 技术栈适配

本工作流适用于多种技术栈：

| 技术栈 | 适配说明 |
|--------|----------|
| PHP | 使用 `SITE_URL` 处理路径，PDO 预处理语句 |
| Node.js | 使用路径模块，参数化查询 |
| Python | 使用 ORM，路径处理 |
| 通用 | 遵循对应语言的编码规范 |

## 维护说明

* 当需要新增或修改角色时，请同步更新本表格及 `.claude/agents/` 下的对应文件。
* 任何 Agent 在启动时，应先读取本文件以了解团队全局结构。
