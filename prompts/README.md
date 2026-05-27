---
model: MiniMax-M2.7
---
# 角色激活提示词模板

> 使用这些模板快速激活对应的AI角色

## 快速索引


| 场景         | 角色       | 模板文件                                                                                       |
| ------------ | ---------- | ---------------------------------------------------------------------------------------------- |
| 新需求启动   | 项目经理   | [project-manager-prompts.md](./project-manager-prompts.md#场景1：新需求启动)                   |
| 设计阶段推进 | 项目经理   | [project-manager-prompts.md](./project-manager-prompts.md#场景2：需求评审通过，推进到设计阶段) |
| 开发阶段推进 | 项目经理   | [project-manager-prompts.md](./project-manager-prompts.md#场景3：设计评审通过，推进到开发阶段) |
| 功能开发     | 开发工程师 | [developer-prompts.md](./developer-prompts.md#场景1：开始功能开发)                             |
| Bug修复      | 开发工程师 | [developer-prompts.md](./developer-prompts.md#场景2：修复Bug)                                  |
| 编写PRD      | 需求分析师 | [prd-writer-prompts.md](./prd-writer-prompts.md#场景1：编写PRD)                                |
| PRD评审      | 需求评审员 | [prd-reviewer-prompts.md](./prd-reviewer-prompts.md#场景1：PRD文档评审)                       |
| 技术设计     | 架构师     | [architect-prompts.md](./architect-prompts.md#场景1：技术设计)                                 |
| 代码审查     | 代码评审员 | [code-reviewer-prompts.md](./code-reviewer-prompts.md#场景1：代码审查)                         |
| 编写测试     | QA工程师   | [qa-engineer-prompts.md](./qa-engineer-prompts.md#场景1：编写测试用例)                         |

## 使用方法

1. 选择对应角色的场景模板
2. 替换方括号 `[...]` 中的内容为具体信息
3. 使用完整的提示词激活角色

## 示例

**激活项目经理（新需求）**：

```
你是项目经理（Project Manager）。请开始管理新项目。

## 用户需求
优化解决方案页面的图片显示，根据图片尺寸动态调整显示效果

请按照工作流程推进：需求启动 -> 需求评审 -> 计划制定 -> 设计评审 -> 开发 -> 代码评审 -> 测试 -> 重构验证
```
