---
AIGC:
    Label: "1"
    ContentProducer: 001191440300708461136T1XGW3
    ProduceID: 10ff82c76385fb81fe56f4a0b12d2d68_e483912ca3c811f192a2525400287e28
    ReservedCode1: zpKJQVAkd6X29n4ZB9WkRg4LPPafOa9gYGyQFLCzK278huc3zlKyZ6TZmCEmYFUaDE7K0Mo687oRl2pldv89st8GIX0BSwg6rm6t7Qi2cFGVxW8FW6tMBlLtQQDy0dIz3iETJaEyikpqGXIFtOt6NTc6v+EwK6dQu10xT4/4Qdo6k7wHH5NMD0XqTnU=
    ContentPropagator: 001191440300708461136T1XGW3
    PropagateID: 10ff82c76385fb81fe56f4a0b12d2d68_e483912ca3c811f192a2525400287e28
    ReservedCode2: zpKJQVAkd6X29n4ZB9WkRg4LPPafOa9gYGyQFLCzK278huc3zlKyZ6TZmCEmYFUaDE7K0Mo687oRl2pldv89st8GIX0BSwg6rm6t7Qi2cFGVxW8FW6tMBlLtQQDy0dIz3iETJaEyikpqGXIFtOt6NTc6v+EwK6dQu10xT4/4Qdo6k7wHH5NMD0XqTnU=
---

# 项目文档（PROJECT.md）

## 项目概述
- 项目名称：电子记课表（教师节礼物）
- 目标用户：初中老师（对电脑不熟悉的"老年"用户，需极简易用）
- 工作区：G:\周课表
- 定位：编程产物礼物，核心是"电子记课表 + 智能换课"，后续做轻量 exe 壳与自动更新

## 开发注意事项（引用 AGENT.md）
1. 每次改动完成后，都必须创建一个对应的 Git commit，以便后续追踪与回滚
2. 每次改动后，都必须编写或更新相关测试，并在交互给用户前，确保所有测试和验证全部通过

## 决策记录（按时间倒序）
### 2026-08-30
- 周循环方案：课表默认"每周同一张"，当前天高亮（不滚动复制多周）。[已确认]
- 交付形态：待定（单文件 HTML 直接交付 vs 基于参考 HTML 升级 vs 从零重写）。
- MVP 功能清单：待确认。
- 不做自动更新，当前版即完整体。[已确认]
- 祝福语定位为"每日祝福"而非教师节祝福，去教师节日化、界面仅显示姓氏。[已确认]
- 新增"简易表格模式（Excel 感 UI）"需求待办。[新需求·高优先级]

## 待办 / 待确认
- [ ] MVP 功能清单最终确认
- [ ] 交付形态确认（基于参考 HTML 升级 或 从零重写）
- [ ] 老师电脑系统确认（影响是否做 exe 壳）
*（内容由AI生成，仅供参考）*
