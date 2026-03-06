"use client";

import React from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";

function wrapXmlLikeBlocks(input: string) {
  const tags = [
    "INSTRUCTIONS",
    "environment_context",
    "turn_aborted",
    "permissions",
    "collaboration_mode",
  ];

  const fence = "````";
  let out = input;
  for (const tag of tags) {
    const re = new RegExp(`<${tag}>[\\s\\S]*?<\\/${tag}>`, "g");
    out = out.replace(re, (m) => `\n${fence}xml\n${m}\n${fence}\n`);
  }
  return out;
}

export function MarkdownMessage(props: { text: string }) {
  const text = React.useMemo(() => wrapXmlLikeBlocks(props.text), [props.text]);

  return (
    <div className="prose prose-invert max-w-none prose-p:my-2 prose-pre:my-0 prose-pre:bg-transparent prose-code:before:content-[''] prose-code:after:content-['']">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a({ href, children, ...rest }) {
            const safeHref = typeof href === "string" ? href : "#";
            return (
              <a
                href={safeHref}
                target={safeHref.startsWith("http") ? "_blank" : undefined}
                rel={safeHref.startsWith("http") ? "noreferrer" : undefined}
                className="underline decoration-muted-foreground/60 underline-offset-4 hover:text-foreground"
                {...rest}
              >
                {children}
              </a>
            );
          },
          blockquote({ children, ...rest }) {
            return (
              <blockquote
                className="my-2 border-l-2 border-border/70 pl-4 text-foreground/90"
                {...rest}
              >
                {children}
              </blockquote>
            );
          },
          table({ children, ...rest }) {
            return (
              <div className="my-3 overflow-auto rounded-lg border">
                <table className="w-full text-sm" {...rest}>
                  {children}
                </table>
              </div>
            );
          },
          th({ children, ...rest }) {
            return (
              <th
                className="border-b bg-muted/30 px-3 py-2 text-left text-xs font-semibold text-foreground/80"
                {...rest}
              >
                {children}
              </th>
            );
          },
          td({ children, ...rest }) {
            return (
              <td className="border-b px-3 py-2 align-top" {...rest}>
                {children}
              </td>
            );
          },
          code({ className, children, ...rest }) {
            const match = /language-(\w+)/.exec(className || "");
            const lang = match?.[1] ?? undefined;
            const code = String(children ?? "");
            const isBlock = code.includes("\n");

            if (!isBlock) {
              return (
                <code
                  className="rounded bg-muted/50 px-1.5 py-0.5 font-mono text-[0.92em]"
                  {...rest}
                >
                  {children}
                </code>
              );
            }

            return (
              <div className="my-2 overflow-hidden rounded-lg border bg-[#0d1117]">
                <SyntaxHighlighter
                  language={lang}
                  style={vscDarkPlus}
                  customStyle={{
                    margin: 0,
                    padding: "12px",
                    background: "transparent",
                    fontSize: "12px",
                    lineHeight: "1.55",
                  }}
                >
                  {code.replace(/\n$/, "")}
                </SyntaxHighlighter>
              </div>
            );
          },
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}
