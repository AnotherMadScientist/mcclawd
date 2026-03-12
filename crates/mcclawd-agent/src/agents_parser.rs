/// Parsed from AGENTS.md markdown.
#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub id: String,
    pub specialty: Option<String>,
    pub model: Option<String>,
    pub tools: Vec<String>,
    pub skills: Vec<String>,
    pub delegate_when: Option<String>,
}

/// A predefined swarm pattern from `## Swarm Patterns` in AGENTS.md.
#[derive(Debug, Clone)]
pub struct SwarmPattern {
    pub name: String,
    pub triggers: Vec<String>,
    pub waves: Vec<WaveTemplate>,
    pub max_replan_depth: usize,
    pub merge_strategy: String,
}

/// A wave template — role + count, not yet filled with specific prompts.
#[derive(Debug, Clone)]
pub struct WaveTemplate {
    pub role: String,
    pub count: WorkerCount,
    pub replan: bool,
    pub skills_override: Vec<String>,
}

/// How many workers to spawn for a wave.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkerCount {
    /// Fixed number of workers (e.g. "× 3").
    Fixed(usize),
    /// One worker per input item (e.g. "× N (one per file)").
    PerInput,
}

#[derive(Debug, Clone)]
pub struct AgentsConfig {
    pub default_skills: Vec<String>,
    pub agents: Vec<AgentSpec>,
    pub delegation_rules: Vec<String>,
    pub swarm_patterns: Vec<SwarmPattern>,
    pub raw_markdown: String,
}

impl AgentsConfig {
    pub fn parse(markdown: &str) -> Self {
        let mut default_skills = vec![];
        let mut agents = vec![];
        let mut delegation_rules = vec![];
        let mut swarm_patterns = vec![];
        let mut current_agent: Option<AgentSpec> = None;
        let mut current_pattern: Option<SwarmPattern> = None;
        let mut section = Section::None;
        let mut sub_field = SubField::None;
        let mut pattern_field = PatternField::None;

        for line in markdown.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("## Default Skills") {
                flush_agent(&mut current_agent, &mut agents);
                flush_pattern(&mut current_pattern, &mut swarm_patterns);
                section = Section::DefaultSkills;
                sub_field = SubField::None;
                pattern_field = PatternField::None;
                continue;
            }
            if trimmed.starts_with("## Available Agents") {
                flush_agent(&mut current_agent, &mut agents);
                flush_pattern(&mut current_pattern, &mut swarm_patterns);
                section = Section::Agents;
                sub_field = SubField::None;
                pattern_field = PatternField::None;
                continue;
            }
            if trimmed.starts_with("## Delegation Rules") {
                flush_agent(&mut current_agent, &mut agents);
                flush_pattern(&mut current_pattern, &mut swarm_patterns);
                section = Section::DelegationRules;
                sub_field = SubField::None;
                pattern_field = PatternField::None;
                continue;
            }
            if trimmed.starts_with("## Swarm Patterns") {
                flush_agent(&mut current_agent, &mut agents);
                flush_pattern(&mut current_pattern, &mut swarm_patterns);
                section = Section::SwarmPatterns;
                sub_field = SubField::None;
                pattern_field = PatternField::None;
                continue;
            }
            if trimmed.starts_with("## ") {
                flush_agent(&mut current_agent, &mut agents);
                flush_pattern(&mut current_pattern, &mut swarm_patterns);
                section = Section::None;
                sub_field = SubField::None;
                pattern_field = PatternField::None;
                continue;
            }

            // New agent heading (### <id>)
            if trimmed.starts_with("### ") && section == Section::Agents {
                flush_agent(&mut current_agent, &mut agents);
                let id = trimmed.trim_start_matches("### ").trim().to_lowercase();
                current_agent = Some(AgentSpec {
                    id,
                    specialty: None,
                    model: None,
                    tools: vec![],
                    skills: vec![],
                    delegate_when: None,
                });
                sub_field = SubField::None;
                continue;
            }

            // New swarm pattern heading (### <name>)
            if trimmed.starts_with("### ") && section == Section::SwarmPatterns {
                flush_pattern(&mut current_pattern, &mut swarm_patterns);
                let name = trimmed.trim_start_matches("### ").trim().to_lowercase();
                current_pattern = Some(SwarmPattern {
                    name,
                    triggers: vec![],
                    waves: vec![],
                    max_replan_depth: 0,
                    merge_strategy: "Concatenate".to_string(),
                });
                pattern_field = PatternField::None;
                continue;
            }

            // Parse bullet items
            if let Some(item) = trimmed.strip_prefix("- ") {
                match section {
                    Section::DefaultSkills => {
                        default_skills.push(item.trim().to_string());
                    }
                    Section::DelegationRules => {
                        delegation_rules.push(item.trim().to_string());
                    }
                    Section::Agents => {
                        if let Some(ref mut agent) = current_agent {
                            if let Some(val) = item.strip_prefix("**Specialty:**") {
                                agent.specialty = Some(val.trim().to_string());
                                sub_field = SubField::None;
                            } else if let Some(val) = item.strip_prefix("**Model:**") {
                                agent.model = Some(val.trim().to_string());
                                sub_field = SubField::None;
                            } else if item.starts_with("**Tools:**") {
                                let val =
                                    item.strip_prefix("**Tools:**").unwrap_or("").trim();
                                if !val.is_empty() {
                                    agent.tools = val
                                        .split(',')
                                        .map(|s| s.trim().to_string())
                                        .collect();
                                }
                                sub_field = SubField::Tools;
                            } else if item.starts_with("**Skills:**") {
                                sub_field = SubField::Skills;
                            } else if let Some(val) =
                                item.strip_prefix("**Delegate when:**")
                            {
                                agent.delegate_when = Some(val.trim().to_string());
                                sub_field = SubField::None;
                            } else {
                                match sub_field {
                                    SubField::Skills => {
                                        agent.skills.push(item.trim().to_string())
                                    }
                                    SubField::Tools => {
                                        agent.tools.push(item.trim().to_string())
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    Section::SwarmPatterns => {
                        if let Some(ref mut pattern) = current_pattern {
                            if let Some(val) = item.strip_prefix("**Trigger:**") {
                                pattern.triggers = val
                                    .split(',')
                                    .map(|s| s.trim().trim_matches('"').to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                                pattern_field = PatternField::None;
                            } else if item.starts_with("**Waves:**") {
                                pattern_field = PatternField::Waves;
                            } else if let Some(val) = item.strip_prefix("**Replan:**") {
                                pattern.max_replan_depth =
                                    parse_replan_depth(val.trim());
                                pattern_field = PatternField::None;
                            } else if let Some(val) = item.strip_prefix("**Merge:**") {
                                pattern.merge_strategy = val.trim().to_string();
                                pattern_field = PatternField::None;
                            } else if pattern_field == PatternField::Waves {
                                // skip — waves are numbered lines, not bullets
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Parse numbered wave lines (1. role × count ...)
            if section == Section::SwarmPatterns
                && pattern_field == PatternField::Waves
            {
                if let Some(wave) = parse_wave_line(trimmed) {
                    if let Some(ref mut pattern) = current_pattern {
                        pattern.waves.push(wave);
                    }
                }
            }
        }

        flush_agent(&mut current_agent, &mut agents);
        flush_pattern(&mut current_pattern, &mut swarm_patterns);

        Self {
            default_skills,
            agents,
            delegation_rules,
            swarm_patterns,
            raw_markdown: markdown.to_string(),
        }
    }

    pub fn skills_for(&self, agent_id: &str) -> Vec<String> {
        let mut skills = self.default_skills.clone();
        if let Some(agent) = self.agents.iter().find(|a| a.id == agent_id) {
            skills.extend(agent.skills.clone());
        }
        skills.dedup();
        skills
    }

    pub fn agent_spec(&self, agent_id: &str) -> Option<&AgentSpec> {
        self.agents.iter().find(|a| a.id == agent_id)
    }

    /// Find the best matching swarm pattern for a prompt, if any.
    /// Returns the first pattern where any trigger keyword appears in the prompt.
    pub fn match_swarm_pattern(&self, prompt: &str) -> Option<&SwarmPattern> {
        let lower = prompt.to_lowercase();
        self.swarm_patterns
            .iter()
            .find(|p| p.triggers.iter().any(|t| lower.contains(&t.to_lowercase())))
    }

    /// Get a swarm pattern by name.
    pub fn swarm_pattern(&self, name: &str) -> Option<&SwarmPattern> {
        self.swarm_patterns.iter().find(|p| p.name == name)
    }
}

#[derive(PartialEq)]
enum Section {
    None,
    DefaultSkills,
    Agents,
    DelegationRules,
    SwarmPatterns,
}

#[derive(PartialEq)]
enum SubField {
    None,
    Skills,
    Tools,
}

#[derive(PartialEq)]
enum PatternField {
    None,
    Waves,
}

fn flush_agent(current: &mut Option<AgentSpec>, agents: &mut Vec<AgentSpec>) {
    if let Some(agent) = current.take() {
        agents.push(agent);
    }
}

fn flush_pattern(current: &mut Option<SwarmPattern>, patterns: &mut Vec<SwarmPattern>) {
    if let Some(pattern) = current.take() {
        patterns.push(pattern);
    }
}

/// Parse "up to 3 rounds on gaps" → 3, or "3" → 3.
fn parse_replan_depth(s: &str) -> usize {
    // Extract first number from the string
    s.split_whitespace()
        .find_map(|word| word.trim_matches(|c: char| !c.is_ascii_digit()).parse().ok())
        .unwrap_or(0)
}

/// Parse a numbered wave line like:
///   "1. researcher × 3 (parallel, different search angles)"
///   "2. analyst (extract findings, identify gaps)"
///   "3. researcher × N (one per gap) [replan]"
fn parse_wave_line(line: &str) -> Option<WaveTemplate> {
    // Must start with a digit followed by '.'
    let trimmed = line.trim();
    let after_num = trimmed
        .strip_prefix(|c: char| c.is_ascii_digit())?;
    let after_dot = after_num
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .strip_prefix('.')?
        .trim();

    if after_dot.is_empty() {
        return None;
    }

    let replan = after_dot.contains("[replan]");
    let clean = after_dot.replace("[replan]", "");
    let clean = clean.trim();

    // Extract role (first word)
    let role = clean.split_whitespace().next()?.to_lowercase();

    // Check for "× N" or "× 3" pattern
    let count = if clean.contains("× N") || clean.contains("x N") {
        WorkerCount::PerInput
    } else if let Some(pos) = clean.find('×').or_else(|| clean.find(" x ")) {
        let after = &clean[pos + '×'.len_utf8()..].trim_start();
        after
            .split_whitespace()
            .next()
            .and_then(|n| n.parse().ok())
            .map(WorkerCount::Fixed)
            .unwrap_or(WorkerCount::Fixed(1))
    } else {
        WorkerCount::Fixed(1)
    };

    // Extract skills override from [...] (not [replan])
    let skills_override = if let Some(start) = clean.find('[') {
        let bracket_content = &clean[start + 1..];
        if let Some(end) = bracket_content.find(']') {
            let content = &bracket_content[..end];
            if content != "replan" {
                content.split(',').map(|s| s.trim().to_string()).collect()
            } else {
                vec![]
            }
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    Some(WaveTemplate {
        role,
        count,
        replan,
        skills_override,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_agents_md() {
        let md = r#"# Agents

## Default Skills
- memory-management
- task-status

## Available Agents

### coding
- **Specialty:** Code generation, debugging
- **Model:** claude-sonnet-4-5
- **Tools:** exec, read, write
- **Skills:**
  - git-workflow
  - code-review
- **Delegate when:** User asks for code changes

### research
- **Specialty:** Deep research
- **Model:** claude-opus-4-5
- **Skills:**
  - academic-search

## Delegation Rules
- Always confirm before delegating
"#;

        let config = AgentsConfig::parse(md);
        assert_eq!(
            config.default_skills,
            vec!["memory-management", "task-status"]
        );
        assert_eq!(config.agents.len(), 2);

        let coding = config.agent_spec("coding").unwrap();
        assert_eq!(coding.model.as_deref(), Some("claude-sonnet-4-5"));
        assert!(coding.skills.contains(&"git-workflow".to_string()));

        let skills = config.skills_for("coding");
        assert!(skills.contains(&"memory-management".to_string()));
        assert!(skills.contains(&"git-workflow".to_string()));

        assert_eq!(config.delegation_rules.len(), 1);
        assert!(config.swarm_patterns.is_empty());
    }

    #[test]
    fn test_parse_swarm_patterns() {
        let md = r#"# Agents

## Available Agents

### researcher
- **Specialty:** Web research
- **Model:** claude-haiku-4-5-20251001
- **Skills:**
  - web-search

### analyst
- **Specialty:** Analysis
- **Model:** claude-sonnet-4-20250514

## Swarm Patterns

### deep-research
- **Trigger:** research, investigate, survey, compare
- **Waves:**
  1. researcher × 3 (parallel, different search angles)
  2. analyst (extract findings, identify gaps)
  3. researcher × N (one per gap) [replan]
  4. analyst (synthesize with citations)
- **Replan:** up to 3 rounds on gaps
- **Merge:** LlmSynthesis

### code-review
- **Trigger:** review, audit
- **Waves:**
  1. coder (read and analyze code)
  2. researcher (search for known issues)
  3. analyst (write review)
- **Merge:** Concatenate
"#;

        let config = AgentsConfig::parse(md);
        assert_eq!(config.agents.len(), 2);
        assert_eq!(config.swarm_patterns.len(), 2);

        // Check deep-research pattern
        let dr = config.swarm_pattern("deep-research").unwrap();
        assert_eq!(dr.triggers, vec!["research", "investigate", "survey", "compare"]);
        assert_eq!(dr.waves.len(), 4);
        assert_eq!(dr.max_replan_depth, 3);
        assert_eq!(dr.merge_strategy, "LlmSynthesis");

        // Wave 1: researcher × 3
        assert_eq!(dr.waves[0].role, "researcher");
        assert_eq!(dr.waves[0].count, WorkerCount::Fixed(3));
        assert!(!dr.waves[0].replan);

        // Wave 2: analyst × 1
        assert_eq!(dr.waves[1].role, "analyst");
        assert_eq!(dr.waves[1].count, WorkerCount::Fixed(1));

        // Wave 3: researcher × N [replan]
        assert_eq!(dr.waves[2].role, "researcher");
        assert_eq!(dr.waves[2].count, WorkerCount::PerInput);
        assert!(dr.waves[2].replan);

        // Wave 4: analyst × 1
        assert_eq!(dr.waves[3].role, "analyst");
        assert_eq!(dr.waves[3].count, WorkerCount::Fixed(1));

        // Check code-review pattern
        let cr = config.swarm_pattern("code-review").unwrap();
        assert_eq!(cr.triggers, vec!["review", "audit"]);
        assert_eq!(cr.waves.len(), 3);
        assert_eq!(cr.max_replan_depth, 0);
        assert_eq!(cr.merge_strategy, "Concatenate");
    }

    #[test]
    fn test_match_swarm_pattern() {
        let md = r#"## Swarm Patterns

### deep-research
- **Trigger:** research, investigate, compare
- **Waves:**
  1. researcher × 3
- **Merge:** LlmSynthesis

### code-review
- **Trigger:** review, audit
- **Waves:**
  1. coder
- **Merge:** Concatenate
"#;

        let config = AgentsConfig::parse(md);

        let m1 = config.match_swarm_pattern("Research how LLM routing works");
        assert!(m1.is_some());
        assert_eq!(m1.unwrap().name, "deep-research");

        let m2 = config.match_swarm_pattern("Review this code for security issues");
        assert!(m2.is_some());
        assert_eq!(m2.unwrap().name, "code-review");

        let m3 = config.match_swarm_pattern("Write a hello world program");
        assert!(m3.is_none());
    }

    #[test]
    fn test_local_model_in_agent_spec() {
        let md = r#"## Available Agents

### coder
- **Model:** ollama/deepseek-coder-v2:33b
- **Skills:**
  - filesystem
"#;

        let config = AgentsConfig::parse(md);
        let coder = config.agent_spec("coder").unwrap();
        assert_eq!(coder.model.as_deref(), Some("ollama/deepseek-coder-v2:33b"));
    }

    #[test]
    fn test_parse_wave_line() {
        let w1 = parse_wave_line("  1. researcher × 3 (parallel)").unwrap();
        assert_eq!(w1.role, "researcher");
        assert_eq!(w1.count, WorkerCount::Fixed(3));
        assert!(!w1.replan);

        let w2 = parse_wave_line("  3. researcher × N (one per gap) [replan]").unwrap();
        assert_eq!(w2.role, "researcher");
        assert_eq!(w2.count, WorkerCount::PerInput);
        assert!(w2.replan);

        let w3 = parse_wave_line("  2. analyst (extract findings)").unwrap();
        assert_eq!(w3.role, "analyst");
        assert_eq!(w3.count, WorkerCount::Fixed(1));

        assert!(parse_wave_line("not a wave").is_none());
    }

    #[test]
    fn test_parse_wave_with_skills_override() {
        let w = parse_wave_line("  1. coder × N (one per file) [langextract]").unwrap();
        assert_eq!(w.role, "coder");
        assert_eq!(w.count, WorkerCount::PerInput);
        assert_eq!(w.skills_override, vec!["langextract"]);
        // [langextract] is skills override, not replan
        assert!(!w.replan);
    }

    #[test]
    fn test_parse_replan_depth() {
        assert_eq!(parse_replan_depth("up to 3 rounds on gaps"), 3);
        assert_eq!(parse_replan_depth("5"), 5);
        assert_eq!(parse_replan_depth("none"), 0);
    }
}
