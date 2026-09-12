/**
 * 对话路径（ChatArea）实际下发给模型的内置工具定义。
 *
 * ## 为什么单独成模块
 * 之前这些定义直接写在 `ChatArea.tsx` 里，而右侧面板的「工具」开关是**另写一份**
 * 固定文案。两者一对不上就出问题：面板上曾长期摆着「代码执行」「网页搜索」两个
 * 开关，可这里**一个对应工具都没有** —— 开关可以拨动，模型却永远拿不到那种能力，
 * 而且三个开关实际只影响同一个布尔值。
 *
 * 现在工具定义集中在此处并导出名称，界面文案由 `AGENT_TOOL_NAMES` 派生，
 * 结构上杜绝再次跑偏（`agent-tools.test.ts` 里有对应守卫）。
 *
 * ## 安全约束（修改前必读）
 * 出于安全设计，这里**不提供任何命令执行或网络访问能力**。
 * 所有文件操作都必须经由 Rust 权限层，按 Agent 独立白名单逐次校验；
 * 因此新增工具时不得绕过该层，也不得加入 shell / 网络类工具。
 */

/** OpenAI 兼容的函数工具描述。 */
export interface AgentToolSpec {
  type: "function";
  function: {
    name: string;
    description: string;
    parameters: {
      type: "object";
      properties: Record<string, { type: string; description: string }>;
      required: string[];
    };
  };
}

export const AGENT_TOOLS: AgentToolSpec[] = [
  {
    type: "function",
    function: {
      name: "list_dir",
      description:
        "列出指定目录下的文件与子目录。仅允许访问该 Agent 的工作区或用户已授权的目录；未授权时返回提示，需请用户在授权弹窗中放行。",
      parameters: {
        type: "object",
        properties: {
          path: { type: "string", description: "目录路径" },
        },
        required: ["path"],
      },
    },
  },
  {
    type: "function",
    function: {
      name: "read_file",
      description:
        "读取文本文件内容（UTF-8）。仅允许读取该 Agent 的工作区或用户已授权的路径；未授权时返回提示，需请用户放行。",
      parameters: {
        type: "object",
        properties: {
          path: { type: "string", description: "文件路径" },
        },
        required: ["path"],
      },
    },
  },
  {
    type: "function",
    function: {
      name: "write_file",
      description:
        "写入文本文件（UTF-8，覆盖写，父目录不存在时自动创建）。仅允许写入该 Agent 的工作区或用户已授权的路径；未授权时返回提示，需请用户放行。",
      parameters: {
        type: "object",
        properties: {
          path: { type: "string", description: "文件路径" },
          content: { type: "string", description: "要写入的完整文本内容" },
        },
        required: ["path", "content"],
      },
    },
  },
];

/** 工具名清单（顺序即定义顺序）。界面文案应从这里派生。 */
export const AGENT_TOOL_NAMES: string[] = AGENT_TOOLS.map((t) => t.function.name);

/**
 * 明确禁止出现在内置工具里的名字。
 * 这不是「暂时没做」，而是安全边界：出现在这里即为回归缺陷。
 */
export const FORBIDDEN_TOOL_NAMES: readonly string[] = [
  "run_command",
  "exec",
  "shell",
  "spawn",
  "bash",
  "powershell",
  "web_search",
  "search_web",
  "http_request",
  "fetch_url",
];
