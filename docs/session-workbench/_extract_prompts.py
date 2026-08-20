import json
import os

p = r"C:\Users\63036\.claude\projects\D--code-revisiting-work-repo-audit-repos-codeg\004e7a7d-7007-4bee-8c8f-9ca6c5e93c73.jsonl"
out = r"D:\code\revisiting\work\repo_audit\repos\codeg\docs\session-workbench\_extract_user_prompts.jsonl"
summ = r"D:\code\revisiting\work\repo_audit\repos\codeg\docs\session-workbench\_extract_user_prompts.md"


def parse_blocks(content):
    texts = []
    images = 0
    is_tool = False
    is_cmd = False
    if isinstance(content, str):
        return [content], 0, False, False
    if not isinstance(content, list):
        return [], 0, False, False
    for b in content:
        if not isinstance(b, dict):
            continue
        t = b.get("type")
        if t in ("tool_result", "tool_use"):
            is_tool = True
        if t == "text":
            tx = b.get("text") or ""
            texts.append(tx)
            if "<command-name>" in tx or "<local-command-stdout>" in tx:
                is_cmd = True
        if t in ("image", "input_image"):
            images += 1
        elif isinstance(b.get("source"), dict) and t in (None, "image"):
            images += 1
    return texts, images, is_tool, is_cmd


prompts = []
with open(p, "r", encoding="utf-8") as f:
    for line in f:
        o = json.loads(line)
        if o.get("type") != "user":
            continue
        if o.get("isMeta"):
            continue
        msg = o.get("message") or {}
        content = msg.get("content") if isinstance(msg, dict) else None
        texts, images, is_tool, is_cmd = parse_blocks(content)
        if is_tool or is_cmd:
            continue
        text = "\n".join(texts).strip()
        if not text and images == 0:
            continue
        if text.startswith("<local-command-caveat>") or text.startswith(
            "<command-name>"
        ):
            continue
        if text.startswith("<task-notification>") or "<task-notification>" in text[:80]:
            continue
        if text.startswith("Another Claude session sent a message"):
            continue
        if "<teammate-message" in text[:200] and "idle_notification" in text:
            continue
        prompts.append(
            {
                "ts": o.get("timestamp"),
                "uuid": o.get("uuid"),
                "images": images,
                "nchars": len(text),
                "text": text[:6000],
            }
        )

print("real human prompts", len(prompts))
print("span", prompts[0]["ts"], "->", prompts[-1]["ts"])
last = prompts[-100:]
print("last100", last[0]["ts"], "->", last[-1]["ts"])
print("with images", sum(1 for x in last if x["images"] > 0))

with open(out, "w", encoding="utf-8") as f:
    for x in last:
        f.write(json.dumps(x, ensure_ascii=False) + "\n")

lines = [
    "# Last 100 user prompts from coordinator session 004e7a7d",
    "",
    f"Session total human prompts: {len(prompts)}",
    f"Window: {last[0]['ts']} → {last[-1]['ts']}",
    f"Prompts with images: {sum(1 for x in last if x['images'] > 0)}",
    "",
]
for i, x in enumerate(last, 1):
    one = x["text"].splitlines()[0][:160]
    img = f" [img x{x['images']}]" if x["images"] else ""
    t = (x["ts"] or "")[11:19]
    lines.append(f"{i:03}. `{t}`{img} {one}")
    # include a bit more if first line is short
    body = x["text"].strip()
    if len(body) > 160:
        snippet = body[:800].replace("\r", "")
        lines.append("")
        lines.append("```")
        lines.append(snippet)
        lines.append("```")
        lines.append("")

with open(summ, "w", encoding="utf-8") as f:
    f.write("\n".join(lines))
print("wrote", out, os.path.getsize(out))
print("wrote", summ, os.path.getsize(summ))
