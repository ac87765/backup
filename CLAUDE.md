# CLAUDE.md

项目规则见 [AGENTS.md](AGENTS.md)（实现方与审查方共用同一份规则）。

@AGENTS.md

## 审查者职责（Claude）

本仓库中 Claude 的角色是设计与审查，不负责主体实现。收到“审查第 N 组 / 某分支”时：
- 对照 `openspec/changes/<change>/specs` 的每个 Scenario 检查是否有实现与测试覆盖；
- 对照 design.md 决策与 AGENTS.md 硬性规则逐条检查偏离；
- 实际运行 AGENTS.md 中的检查命令，以运行结果为准；
- 按严重程度输出问题清单（阻塞 / 应修 / 建议），附文件与行号。
