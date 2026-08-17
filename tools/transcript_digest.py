#!/usr/bin/env python3
"""Digest Claude Code / Codex session transcripts into readable text.

Strips a session .jsonl down to the conversational spine: human/user
messages, assistant text, and one-line tool-call summaries. Tool results,
reasoning blocks, attachments and machine bookkeeping are dropped, and
oversized blocks are truncated, so a multi-hundred-MB corpus reduces to
something an agent can actually read.

Usage:
  transcript_digest.py FILE.jsonl              # digest one file to stdout
  transcript_digest.py --corpus LIST OUTDIR    # digest every file listed in
                                               # LIST (one path per line) into
                                               # OUTDIR/digests/ and split the
                                               # result into OUTDIR/chunks/
"""

import json
import os
import sys

USER_LIMIT = 8000
ASSISTANT_LIMIT = 5000
TOOL_LIMIT = 300
CHUNK_LIMIT = 120_000  # chars per chunk file (~30k tokens)


def truncate(text, limit):
    text = text.strip()
    if len(text) <= limit:
        return text
    return text[:limit] + f"\n[... truncated, {len(text) - limit} more chars]"


def content_blocks(content):
    if isinstance(content, str):
        return [{"type": "text", "text": content}]
    if isinstance(content, list):
        return [b for b in content if isinstance(b, dict)]
    return []


def digest_claude(path):
    out = []
    for line in open(path, errors="replace"):
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            continue
        kind = entry.get("type")
        if kind not in ("user", "assistant"):
            continue
        message = entry.get("message") or {}
        for block in content_blocks(message.get("content")):
            btype = block.get("type")
            if btype == "text":
                text = block.get("text", "")
                if not text.strip():
                    continue
                if kind == "user":
                    if text.lstrip().startswith("<local-command"):
                        continue
                    out.append("USER:\n" + truncate(text, USER_LIMIT))
                else:
                    out.append("ASSISTANT:\n" + truncate(text, ASSISTANT_LIMIT))
            elif btype == "tool_use" and kind == "assistant":
                name = block.get("name", "?")
                args = json.dumps(block.get("input", {}), ensure_ascii=False)
                out.append(f"TOOL {name}: {truncate(args, TOOL_LIMIT)}")
    return "\n\n".join(out)


def digest_codex(path):
    out = []
    for line in open(path, errors="replace"):
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            continue
        if entry.get("type") != "response_item":
            continue
        payload = entry.get("payload") or {}
        ptype = payload.get("type")
        if ptype == "message":
            role = payload.get("role")
            texts = []
            for block in content_blocks(payload.get("content")):
                text = block.get("text") or block.get("input_text") or ""
                if text.strip():
                    texts.append(text)
            text = "\n".join(texts)
            if not text.strip():
                continue
            if "<environment_context>" in text or "<user_instructions>" in text:
                continue
            if role == "user":
                out.append("USER:\n" + truncate(text, USER_LIMIT))
            elif role == "assistant":
                out.append("ASSISTANT:\n" + truncate(text, ASSISTANT_LIMIT))
        elif ptype in ("function_call", "local_shell_call", "custom_tool_call"):
            name = payload.get("name") or ptype
            args = payload.get("arguments") or json.dumps(
                payload.get("action", {}), ensure_ascii=False
            )
            out.append(f"TOOL {name}: {truncate(str(args), TOOL_LIMIT)}")
    return "\n\n".join(out)


def digest(path):
    with open(path, errors="replace") as handle:
        head = handle.read(4096)
    if '"response_item"' in head or '"session_meta"' in head:
        return digest_codex(path)
    return digest_claude(path)


def slug_for(path):
    parts = path.split(os.sep)
    tail = [p for p in parts[-4:] if p]
    return "-".join(tail).replace(".jsonl", "")[-120:]


def run_corpus(list_path, outdir):
    digest_dir = os.path.join(outdir, "digests")
    chunk_dir = os.path.join(outdir, "chunks")
    os.makedirs(digest_dir, exist_ok=True)
    os.makedirs(chunk_dir, exist_ok=True)

    paths = [p.strip() for p in open(list_path) if p.strip()]
    pieces = []  # (header, text)
    for path in paths:
        text = digest(path)
        if len(text.strip()) < 200:
            continue
        slug = slug_for(path)
        with open(os.path.join(digest_dir, slug + ".txt"), "w") as handle:
            handle.write(text)
        pieces.append((f"=== TRANSCRIPT {slug} ===", text))

    chunk_index = 0
    buffer = []
    size = 0

    def flush():
        nonlocal chunk_index, buffer, size
        if not buffer:
            return
        name = os.path.join(chunk_dir, f"chunk-{chunk_index:03d}.txt")
        with open(name, "w") as handle:
            handle.write("\n\n".join(buffer))
        print(name)
        chunk_index += 1
        buffer = []
        size = 0

    for header, text in pieces:
        # Slice big digests so no chunk exceeds the limit.
        offset = 0
        while offset < len(text):
            piece = text[offset : offset + CHUNK_LIMIT]
            label = header if offset == 0 else header + f" (cont. @{offset})"
            if size + len(piece) > CHUNK_LIMIT:
                flush()
            buffer.append(label + "\n\n" + piece)
            size += len(piece)
            offset += CHUNK_LIMIT
    flush()
    print(
        f"digested {len(pieces)} transcripts into {chunk_index} chunks",
        file=sys.stderr,
    )


def main():
    args = sys.argv[1:]
    if args and args[0] == "--corpus":
        run_corpus(args[1], args[2])
    elif len(args) == 1:
        print(digest(args[0]))
    else:
        print(__doc__)
        sys.exit(1)


main()
