import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

/** Renders assistant replies as GitHub-flavored markdown with compact
 * chat-bubble styling (tight block gaps, ZCode-like density). Raw HTML is
 * not rendered (react-markdown default). Wide tables scroll inside their own
 * wrapper so the page never grows. */
export function Markdown({ text }: { text: string }) {
  return (
    <div className="break-words text-sm leading-5 [&_a]:underline [&_blockquote]:border-l-2 [&_blockquote]:border-border [&_blockquote]:pl-2 [&_code]:rounded [&_code]:bg-background/70 [&_code]:px-1 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-[0.85em] [&_h1]:mb-0.5 [&_h1]:mt-2 [&_h1]:text-[0.95em] [&_h1]:font-bold [&_h2]:mb-0.5 [&_h2]:mt-2 [&_h2]:text-[0.95em] [&_h2]:font-bold [&_h3]:mb-0.5 [&_h3]:mt-1.5 [&_h3]:text-sm [&_h3]:font-bold [&_h4]:mb-0.5 [&_h4]:mt-1.5 [&_h4]:text-sm [&_h4]:font-semibold [&_hr]:my-1.5 [&_hr]:border-border [&_li]:my-0 [&_ol]:my-1 [&_ol]:list-decimal [&_ol]:pl-5 [&_p]:my-1 [&_p:first-child]:mt-0 [&_p:last-child]:mb-0 [&_pre]:my-1 [&_pre]:overflow-x-auto [&_pre]:rounded-md [&_pre]:bg-background/70 [&_pre]:p-2 [&_strong]:font-semibold [&_table]:my-1 [&_table]:w-full [&_table]:text-xs [&_td]:border [&_td]:border-border [&_td]:px-1.5 [&_td]:py-0.5 [&_th]:border [&_th]:border-border [&_th]:bg-background/50 [&_th]:px-1.5 [&_th]:py-0.5 [&_ul]:my-1 [&_ul]:list-disc [&_ul]:pl-5">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          table: ({ children }) => (
            <div className="my-1 overflow-x-auto">
              <table className="w-full border-collapse text-xs [&_td]:border [&_td]:border-border [&_td]:px-1.5 [&_td]:py-0.5 [&_th]:border [&_th]:border-border [&_th]:bg-background/50 [&_th]:px-1.5 [&_th]:py-0.5">
                {children}
              </table>
            </div>
          ),
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}
