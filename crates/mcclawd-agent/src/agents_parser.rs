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

#[derive(Debug, Clone)]
pub struct AgentsConfig {
    pub default_skills: Vec<String>,
    pub agents: Vec<AgentSpec>,
    pub delegation_rules: Vec<String>,
    pub raw_markdown: String,
}

impl AgentsConfig {
    pub fn parse(markdown: &str) -> Self {
        let mut default_skills = vec![];
        let mut agents = vec![];
        let mut delegation_rules = vec![];
        let mut current_agent: Option<AgentSpec> = None;
        let mut section = Section::None;
        let mut sub_field = SubField::None;

        for line in markdown.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("## Default Skills") {
                flush_agent(&mut current_agent, &mut agents);
                section = Section::DefaultSkills;
                sub_field = SubField::None;
                continue;
            }
            if trimmed.starts_with("## Available Agents") {
                flush_agent(&mut current_agent, &mut agents);
                section = Section::Agents;
                sub_field = SubField::None;
                continue;
            }
            if trimmed.starts_with("## Delegation Rules") {
                flush_agent(&mut current_agent, &mut agents);
                section = Section::DelegationRules;
                sub_field = SubField::None;
                continue;
            }
            if trimmed.starts_with("## ") {
                flush_agent(&mut current_agent, &mut agents);
                section = Section::None;
                sub_field = SubField::None;
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
                    _ => {}
                }
            }
        }

        flush_agent(&mut current_agent, &mut agents);

        Self {
            default_skills,
            agents,
            delegation_rules,
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
}

#[derive(PartialEq)]
enum Section {
    None,
    DefaultSkills,
    Agents,
    DelegationRules,
}

#[derive(PartialEq)]
enum SubField {
    None,
    Skills,
    Tools,
}

fn flush_agent(current: &mut Option<AgentSpec>, agents: &mut Vec<AgentSpec>) {
    if let Some(agent) = current.take() {
        agents.push(agent);
    }
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
    }
}
