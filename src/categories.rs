pub const CATEGORIES: &[(&str, &str)] = &[
    ("belief", "观点、看法和人生感悟"),
    ("idea", "想法、灵感和创意"),
    ("plan", "未来计划和准备做的事情"),
    ("activity", "已经完成或正在做的事情"),
    ("mood", "心情、情绪和睡眠"),
    ("reminder", "需要记住、提醒和重要日期"),
    ("health", "生病、症状和身体状况"),
    ("other", "有价值但不属于其他类型的内容"),
];

pub fn is_valid(value: &str) -> bool {
    CATEGORIES.iter().any(|(name, _)| *name == value)
}

pub fn prompt() -> String {
    let choices = CATEGORIES
        .iter()
        .map(|(name, description)| format!("- {name}: {description}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "你是个人记录分类器。为每条记录选择一个或多个最合适的分类。只能使用下面的英文 name，不要创建新分类：\n{choices}\n输出 JSON 对象 {{\"assignments\":[{{\"log_id\":\"...\",\"categories\":[\"...\"]}}]}}。每条记录至少选择一个分类。"
    )
}
