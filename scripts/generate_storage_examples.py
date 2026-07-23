#!/usr/bin/env python3
"""Generate the balanced synthetic seed set used by the local storage gate.

Running this script replaces resources/storage-examples.json. Add personal examples
after generation and do not rerun it unless you intentionally want a fresh seed set.
"""

import json
from pathlib import Path


ZH_STORE_TOPICS = [
    "Rust 日志 Agent", "Telegram 机器人", "个人知识库", "隐私保护方案", "本地 embedding 模型",
    "工作计划", "睡眠习惯", "运动安排", "家庭生活", "学习目标",
]
EN_STORE_TOPICS = [
    "my Rust journal agent", "the Telegram bot", "my personal knowledge base",
    "the privacy design", "the local embedding model", "my work routine",
    "my sleep habits", "my exercise plan", "my family life", "my learning goal",
]
ZH_STORE_TEMPLATES = [
    "今天我推进了{topic}，完成度比昨天高。", "我决定下周优先处理{topic}。",
    "我发现自己对{topic}越来越有兴趣。", "今天关于{topic}的讨论改变了我的想法。",
    "我计划周末认真整理{topic}。", "最近{topic}让我有些焦虑，我想减少压力。",
    "我意识到早上处理{topic}效率最高。", "今天在{topic}上犯了错误，但找到了原因。",
    "我希望未来三个月持续改进{topic}。", "和朋友聊完后，我重新考虑了{topic}。",
    "今天完成{topic}的一个阶段，我很有成就感。", "我准备停止拖延，明天开始处理{topic}。",
    "最近我对{topic}的看法发生了明显变化。", "我给自己定了目标：本月改善{topic}。",
    "今天想到一个可以简化{topic}的新办法。", "我发现{topic}已经影响到我的日常状态。",
    "经过比较，我选择继续投入{topic}。", "今天记录一个教训：处理{topic}前要先做计划。",
    "我想养成每天检查{topic}进度的习惯。", "这周{topic}进展顺利，我决定保持当前节奏。",
    "今天的经历让我确认{topic}对我很重要。", "我暂时搁置了{topic}，等下个月再继续。",
    "我更喜欢用简单、可持续的方式推进{topic}。", "昨天关于{topic}的结果让我有点失望。",
    "我答应自己完成{topic}后要好好休息。",
]
EN_STORE_TEMPLATES = [
    "I made meaningful progress on {topic} today.", "I decided to prioritize {topic} next week.",
    "I have become increasingly interested in {topic}.", "Today's discussion changed how I think about {topic}.",
    "I plan to organize {topic} carefully this weekend.", "I have felt anxious about {topic} lately and want to reduce the pressure.",
    "I realized that I handle {topic} best in the morning.", "I made a mistake with {topic} today but found the cause.",
    "I want to keep improving {topic} over the next three months.", "After talking with a friend, I reconsidered {topic}.",
    "I completed a milestone for {topic} today and felt proud.", "I will stop postponing {topic} and start tomorrow.",
    "My opinion about {topic} has changed noticeably recently.", "I set a personal goal to improve {topic} this month.",
    "I had a new idea today that could simplify {topic}.", "I noticed that {topic} has started affecting my daily mood.",
    "After comparing the options, I chose to continue investing in {topic}.", "A lesson from today is to plan before working on {topic}.",
    "I want to build a habit of checking progress on {topic} every day.", "Progress on {topic} went well this week, so I will keep the same pace.",
    "Today's experience confirmed that {topic} matters to me.", "I paused {topic} for now and will return to it next month.",
    "I prefer a simple and sustainable approach to {topic}.", "Yesterday's result for {topic} left me disappointed.",
    "I promised myself a proper break after finishing {topic}.",
]

ZH_IGNORE_TOPICS = [
    "Rust 所有权", "SQLite 数据库", "Telegram Bot API", "embedding 模型", "GLM 参数",
    "天气预报", "英语翻译", "文件导出", "日志搜索", "终端命令",
]
EN_IGNORE_TOPICS = [
    "Rust ownership", "SQLite databases", "the Telegram Bot API", "embedding models",
    "GLM parameters", "the weather forecast", "English translation", "file export",
    "journal search", "terminal commands",
]
ZH_IGNORE_TEMPLATES = [
    "什么是{topic}？", "请简单解释一下{topic}。", "怎样配置{topic}？", "给我一个{topic}的示例。",
    "帮我查一下{topic}。", "如何开始使用{topic}？", "列出{topic}的常用选项。",
    "{topic}和其他方案有什么区别？", "能否总结一下{topic}的文档？", "告诉我{topic}的基本原理。",
    "请检查{topic}现在是否可用。", "如何排查{topic}的错误？", "把{topic}翻译成英文。",
    "请删除上一条与{topic}有关的记录。", "请显示最近与{topic}有关的五条日志。",
    "请把{topic}相关数据导出成 JSON。", "运行{topic}需要什么命令？", "{topic}支持哪些功能？",
    "帮我生成一段关于{topic}的测试文字。", "测试一下{topic}，不用保存。",
    "你好，你了解{topic}吗？", "请给出{topic}的官方链接。", "{topic}一般需要多少内存？",
    "怎么升级{topic}？", "请比较两种{topic}实现方式。",
]
EN_IGNORE_TEMPLATES = [
    "What is {topic}?", "Please explain {topic} briefly.", "How do I configure {topic}?",
    "Give me an example of {topic}.", "Please look up {topic} for me.", "How do I get started with {topic}?",
    "List the common options for {topic}.", "How does {topic} differ from other approaches?",
    "Can you summarize the documentation for {topic}?", "Tell me the basic principles of {topic}.",
    "Please check whether {topic} is currently available.", "How can I troubleshoot an error involving {topic}?",
    "Translate {topic} into Chinese.", "Delete the previous entry about {topic}.",
    "Show my five most recent logs about {topic}.", "Export the data about {topic} as JSON.",
    "What command runs {topic}?", "What features does {topic} support?",
    "Generate some test text about {topic}.", "Test {topic} without saving anything.",
    "Hello, do you know about {topic}?", "Give me the official link for {topic}.",
    "How much memory does {topic} usually need?", "How do I upgrade {topic}?",
    "Compare two ways to implement {topic}.",
]


def expand(topics: list[str], templates: list[str]) -> list[str]:
    return [template.format(topic=topic) for topic in topics for template in templates]


def main() -> None:
    store = expand(ZH_STORE_TOPICS, ZH_STORE_TEMPLATES) + expand(EN_STORE_TOPICS, EN_STORE_TEMPLATES)
    ignore = expand(ZH_IGNORE_TOPICS, ZH_IGNORE_TEMPLATES) + expand(EN_IGNORE_TOPICS, EN_IGNORE_TEMPLATES)
    assert len(store) == 500 and len(ignore) == 500
    assert len(set(store)) == 500 and len(set(ignore)) == 500
    output = Path(__file__).resolve().parents[1] / "resources" / "storage-examples.json"
    output.write_text(json.dumps({"store": store, "ignore": ignore}, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()
