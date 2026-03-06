//! 状态图构建器。
//!
//! 使用 Builder 模式收集节点、边和通道定义，最终通过 `compile()` 验证
//! 图拓扑的完整性（入口存在、边指向合法、所有节点有出边）并生成不可变的运行时。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::channels::{AnyChannel, ChannelValues, NamedBarrierValue};
use crate::core::error::AgentError;

use super::command::NodeOutput;
use super::compiled::CompiledGraph;
use super::edge::{Edge, RouteDecision, END, START};
use super::node::{FnNode, GraphNode};
use super::state_schema::State;
use super::subgraph::SubgraphNode;
use super::types::{CompileConfig, JoinEdge};

/// 状态图构建器，编译前的可变图定义
pub struct StateGraph {
    channel_specs: HashMap<String, Box<dyn AnyChannel>>,
    nodes: HashMap<String, Box<dyn GraphNode>>,
    edges: HashMap<String, Edge>,
    entry: Option<String>,
    joins: Vec<JoinEdge>,
    input_keys: Option<HashSet<String>>,
    output_keys: Option<HashSet<String>>,
}

impl StateGraph {
    pub fn new(channels: HashMap<String, Box<dyn AnyChannel>>) -> Self {
        Self {
            channel_specs: channels,
            nodes: HashMap::new(),
            edges: HashMap::new(),
            entry: None,
            joins: Vec::new(),
            input_keys: None,
            output_keys: None,
        }
    }

    /// 从实现 `State` trait 的类型创建图，自动生成 channel 规格。
    pub fn from_state<S: State>() -> Self {
        Self::new(S::channels())
    }

    /// 注册一个图节点，节点名由 GraphNode::name() 提供
    pub fn add_node(&mut self, node: impl GraphNode + 'static) -> &mut Self {
        let name = node.name().to_string();
        self.nodes.insert(name, Box::new(node));
        self
    }

    /// 注册一个闭包节点，无需实现 GraphNode trait
    pub fn add_node_fn<F, Fut>(&mut self, name: &str, func: F) -> &mut Self
    where
        F: Fn(ChannelValues, super::node::NodeContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<NodeOutput, crate::core::error::AgentError>>
            + Send
            + 'static,
    {
        self.add_node(FnNode::new(name, func))
    }

    /// 注册一个强类型闭包节点。
    ///
    /// 内部自动将 `ChannelValues` 转换为 `S`，闭包直接接收强类型 state。
    pub fn add_node_fn_typed<S, F, Fut>(&mut self, name: &str, func: F) -> &mut Self
    where
        S: State + 'static,
        F: Fn(S, super::node::NodeContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<NodeOutput, crate::core::error::AgentError>>
            + Send
            + 'static,
    {
        let func = Arc::new(func);
        self.add_node(FnNode::new(name, move |state: ChannelValues, ctx| {
            let typed = S::from_channel_values(&state);
            let func = func.clone();
            async move {
                let s = typed?;
                (func)(s, ctx).await
            }
        }))
    }

    /// 添加静态边：从 from 节点执行完后固定跳转到 to 节点。
    /// 当 `from == START` 时等价于设置入口节点。
    pub fn add_edge(&mut self, from: &str, to: &str) -> &mut Self {
        if from == START {
            self.entry = Some(to.to_string());
            return self;
        }
        let edge = if to == END {
            Edge::End
        } else {
            Edge::Static(to.to_string())
        };
        self.edges.insert(from.to_string(), edge);
        self
    }

    /// 添加条件边：从 from 节点执行完后通过 router 闭包动态决定下一步。
    /// 当 `from == START` 时，构建条件入口：invoke 时先通过 router 决定第一个执行的节点。
    pub fn add_conditional_edge(
        &mut self,
        from: &str,
        router: impl Fn(&ChannelValues) -> RouteDecision + Send + Sync + 'static,
    ) -> &mut Self {
        if from == START {
            self.entry = None;
            self.edges
                .insert(START.to_string(), Edge::Conditional(Box::new(router)));
            return self;
        }
        self.edges
            .insert(from.to_string(), Edge::Conditional(Box::new(router)));
        self
    }

    /// 添加带 pathMap 的条件边：router 返回语义 key，pathMap 将 key 映射到实际节点名。
    /// 当 `from == START` 时，构建条件入口。
    pub fn add_conditional_edge_with_map(
        &mut self,
        from: &str,
        router: impl Fn(&ChannelValues) -> String + Send + Sync + 'static,
        path_map: HashMap<String, String>,
    ) -> &mut Self {
        let edge = Edge::Conditional(Box::new(move |state: &ChannelValues| {
            let key = router(state);
            match path_map.get(&key) {
                Some(node) if node == END => RouteDecision::End,
                Some(node) => RouteDecision::Next(node.clone()),
                None => RouteDecision::End,
            }
        }));

        if from == START {
            self.entry = None;
            self.edges.insert(START.to_string(), edge);
            return self;
        }
        self.edges.insert(from.to_string(), edge);
        self
    }

    /// 注册一个已编译的子图作为节点
    pub fn add_subgraph(&mut self, name: &str, subgraph: CompiledGraph) -> &mut Self {
        self.add_node(SubgraphNode::new(name, Arc::new(subgraph)))
    }

    /// 批量添加节点并自动串联为线性链：A → B → C → ...
    /// 内部自动调用 `add_edge` 连接相邻节点，首尾不连接 START/END。
    pub fn add_sequence(&mut self, nodes: Vec<Box<dyn GraphNode>>) -> &mut Self {
        let names: Vec<String> = nodes.iter().map(|n| n.name().to_string()).collect();
        for node in nodes {
            let name = node.name().to_string();
            self.nodes.insert(name, node);
        }
        for pair in names.windows(2) {
            self.edges
                .insert(pair[0].clone(), Edge::Static(pair[1].clone()));
        }
        self
    }

    /// 多源边（Join）：等待 sources 中所有节点完成后才执行 target。
    /// 内部自动注册一个 NamedBarrierValue 通道，compile 时传递给执行引擎。
    pub fn add_edge_join(&mut self, sources: &[&str], target: &str) -> &mut Self {
        let channel_name = format!("__join:{}:{}", sources.join("+"), target);
        self.channel_specs.insert(
            channel_name.clone(),
            Box::new(NamedBarrierValue::new(sources)),
        );
        self.joins.push(JoinEdge {
            sources: sources.iter().map(|s| s.to_string()).collect(),
            target: target.to_string(),
            channel_name,
        });
        self
    }

    /// 设置 invoke 时接受的输入 key 集合。
    ///
    /// 调用 `invoke` 时，只有 input keys 中的字段会被写入 channels，多余字段被忽略。
    /// 不设置则接受所有字段。
    pub fn with_input_keys(mut self, keys: &[&str]) -> Self {
        self.input_keys = Some(keys.iter().map(|k| k.to_string()).collect());
        self
    }

    /// 设置 invoke 返回的输出 key 集合。
    ///
    /// 图执行完毕后，只返回 output keys 中的字段，内部状态不暴露。
    /// 不设置则返回所有字段。
    pub fn with_output_keys(mut self, keys: &[&str]) -> Self {
        self.output_keys = Some(keys.iter().map(|k| k.to_string()).collect());
        self
    }

    /// 从 `State` trait 的 `keys()` 设置输入 schema。
    pub fn with_input<S: State>(mut self) -> Self {
        self.input_keys = Some(S::keys().into_iter().map(|k| k.to_string()).collect());
        self
    }

    /// 从 `State` trait 的 `keys()` 设置输出 schema。
    pub fn with_output<S: State>(mut self) -> Self {
        self.output_keys = Some(S::keys().into_iter().map(|k| k.to_string()).collect());
        self
    }

    /// 设置图的入口节点。
    /// 推荐使用 `add_edge(START, "node")` 替代此方法。
    #[deprecated(note = "use add_edge(START, \"node\") instead")]
    pub fn set_entry(&mut self, name: &str) -> &mut Self {
        self.entry = Some(name.to_string());
        self
    }

    /// 验证图拓扑并编译为不可变的运行时
    pub fn compile(self, config: CompileConfig) -> Result<CompiledGraph, AgentError> {
        let entry = self.resolve_entry()?;

        if !self.nodes.contains_key(&entry) && !self.edges.contains_key(START) {
            return Err(AgentError::GraphError(format!(
                "Entry node '{}' not found in nodes",
                entry
            )));
        }

        let node_names: HashSet<&str> = self.nodes.keys().map(|s| s.as_str()).collect();
        for (from, edge) in &self.edges {
            if from != START && !node_names.contains(from.as_str()) {
                return Err(AgentError::GraphError(format!(
                    "Edge source '{}' is not a registered node",
                    from
                )));
            }
            if let Edge::Static(to) = edge {
                if !node_names.contains(to.as_str()) {
                    return Err(AgentError::GraphError(format!(
                        "Edge target '{}' is not a registered node",
                        to
                    )));
                }
            }
        }

        // Join 的 source 节点通过 barrier 路由，不需要独立的出边
        let join_sources: HashSet<&str> = self
            .joins
            .iter()
            .flat_map(|j| j.sources.iter().map(|s| s.as_str()))
            .collect();

        for name in &node_names {
            if *name != END
                && !self.edges.contains_key(*name)
                && !join_sources.contains(*name)
            {
                return Err(AgentError::GraphError(format!(
                    "Node '{}' has no outgoing edge",
                    name
                )));
            }
        }

        let interrupt_before: HashSet<String> = config.interrupt_before.into_iter().collect();
        let interrupt_after: HashSet<String> = config.interrupt_after.into_iter().collect();

        let nodes: HashMap<String, Arc<dyn GraphNode>> = self
            .nodes
            .into_iter()
            .map(|(k, v)| (k, Arc::from(v)))
            .collect();

        let mut compiled = CompiledGraph::new(
            self.channel_specs,
            nodes,
            self.edges,
            entry,
            config.checkpointer,
            interrupt_before,
            interrupt_after,
            config.retry_policy,
            self.joins,
        );
        compiled.set_input_keys(self.input_keys);
        compiled.set_output_keys(self.output_keys);
        Ok(compiled)
    }

    /// 从 entry 字段或 START 条件边解析出入口节点名。
    /// 若有 START 条件边，返回占位字符串，运行时再动态决定。
    fn resolve_entry(&self) -> Result<String, AgentError> {
        if let Some(ref entry) = self.entry {
            return Ok(entry.clone());
        }
        if self.edges.contains_key(START) {
            return Ok(START.to_string());
        }
        Err(AgentError::GraphError(
            "No entry node set. Use add_edge(START, \"node\") or set_entry()".into(),
        ))
    }
}
