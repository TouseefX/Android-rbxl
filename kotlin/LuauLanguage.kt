package com.yourname.rbxleditor

import android.os.Bundle
import io.github.rosemoe.sora.lang.EmptyLanguage
import io.github.rosemoe.sora.lang.analysis.AnalyzeManager
import io.github.rosemoe.sora.lang.analysis.SimpleAnalyzeManager
import io.github.rosemoe.sora.lang.completion.CompletionItem
import io.github.rosemoe.sora.lang.completion.CompletionPublisher
import io.github.rosemoe.sora.lang.completion.SimpleCompletionItem
import io.github.rosemoe.sora.lang.smartEnter.NewlineHandleResult
import io.github.rosemoe.sora.lang.smartEnter.NewlineHandler
import io.github.rosemoe.sora.lang.styling.MappedSpans
import io.github.rosemoe.sora.lang.styling.Styles
import io.github.rosemoe.sora.lang.styling.TextStyle
import io.github.rosemoe.sora.text.CharPosition
import io.github.rosemoe.sora.text.Content
import io.github.rosemoe.sora.text.ContentReference
import io.github.rosemoe.sora.widget.SymbolPairMatch
import io.github.rosemoe.sora.widget.schemes.EditorColorScheme
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit

/**
 * Luau support for the sora-editor widget.
 *
 * This replaces the hand-rolled EditText machinery: sora already owns bracket
 * pairing, smart Enter, indentation, the gutter and the completion window, so
 * the only things worth writing here are the parts that are actually
 * Luau-specific -- the token colors, the block rules, and the bridge to the
 * Rust `luau_intelligence` index.
 *
 * Threading is the important detail. [requireAutoComplete] is called by sora on
 * a worker thread and is *allowed* to block, which is what makes it possible to
 * drop the old debounce timer and ListPopupWindow entirely: we ask Rust for
 * suggestions, park until the JNI callback answers, then hand the results
 * straight to the publisher.
 */
class LuauLanguage(
    private val scriptId: Long,
    /** Sends a completion request to Rust; the reply arrives via [deliverCompletions]. */
    private val requestCompletions: (Long, String, Int) -> Unit
) : EmptyLanguage() {

    /**
     * Hand-off point for the JNI reply. Capacity 1 with an explicit drain on
     * each request: a late answer to an abandoned request must never be handed
     * to the next one, or the popup shows suggestions for the previous word.
     */
    private val inbox = ArrayBlockingQueue<List<CompletionItem>>(1)

    @Volatile
    private var generation = 0

    /** Prefix length the in-flight request was made for, used as the replace span. */
    @Volatile
    private var pendingPrefixLength = 0

    /**
     * Called from the UI thread when Rust answers with a completion payload.
     * Parsing happens here so the generation counter never has to leave this
     * class; a reply that arrives after its request was superseded is dropped
     * by the capacity-1 inbox plus the generation check in the waiter.
     */
    fun deliverCompletions(json: String) {
        val items = ArrayList<CompletionItem>()
        try {
            val array = org.json.JSONArray(json)
            for (i in 0 until array.length()) {
                val entry = array.getJSONObject(i)
                val label = entry.optString("label")
                if (label.isEmpty()) continue
                val insert = entry.optString("insertText", label)
                // Rust reports how many characters to replace; fall back to the
                // prefix we measured when the request went out.
                val replace = entry.optInt("replaceChars", pendingPrefixLength)
                items.add(completionItem(label, entry.optString("detail"), insert, replace))
            }
        } catch (_: Exception) {
            return
        }
        inbox.offer(items)
    }

    override fun getAnalyzeManager(): AnalyzeManager = analyzer

    override fun getIndentAdvance(content: ContentReference, line: Int, column: Int): Int {
        val text = content.getLine(line).substring(0, column.coerceAtMost(content.getColumnCount(line)))
        return if (opensLuauBlock(stripLuauComment(text).trimEnd())) INDENT_WIDTH else 0
    }

    /** Spaces, never '\t' -- matches `app::INDENT_WIDTH` on the Rust side. */
    override fun useTab(): Boolean = false

    override fun getSymbolPairs(): SymbolPairMatch = symbolPairs

    override fun getNewlineHandlers(): Array<NewlineHandler> = newlineHandlers

    override fun requireAutoComplete(
        content: ContentReference,
        position: CharPosition,
        publisher: CompletionPublisher,
        extraArguments: Bundle
    ) {
        val prefix = prefixAt(content, position)
        // An empty prefix on a non-member position would match the whole index
        // and is never useful, so skip the round trip entirely.
        if (prefix.isEmpty() && !isMemberAccess(content, position)) return

        val request = ++generation
        pendingPrefixLength = prefix.length
        inbox.clear()
        val source = content.reference.toString()
        requestCompletions(scriptId, source, position.index)

        // Bounded wait: if Rust is slow or the script was closed underneath us,
        // returning empty-handed is better than pinning sora's worker thread.
        // The reply is produced by the Rust event loop, which drains the JNI
        // channel once per frame, so the budget has to cover a few frames
        // rather than just the indexing cost.
        val items = inbox.poll(COMPLETION_TIMEOUT_MS, TimeUnit.MILLISECONDS) ?: return
        if (request != generation) return
        publisher.addItems(items)
        publisher.updateList()
    }

    override fun destroy() {
        generation++
        inbox.clear()
    }

    /** The identifier fragment immediately left of the caret. */
    private fun prefixAt(content: ContentReference, position: CharPosition): String {
        val line = content.getLine(position.line)
        var start = position.column.coerceIn(0, line.length)
        while (start > 0 && (line[start - 1].isLetterOrDigit() || line[start - 1] == '_')) start--
        return line.substring(start, position.column.coerceIn(start, line.length))
    }

    /** Whether the caret sits after a `.` or `:`, where members are offered. */
    private fun isMemberAccess(content: ContentReference, position: CharPosition): Boolean {
        val line = content.getLine(position.line)
        var index = position.column.coerceIn(0, line.length)
        while (index > 0 && (line[index - 1].isLetterOrDigit() || line[index - 1] == '_')) index--
        val previous = line.getOrNull(index - 1)
        return previous == '.' || previous == ':'
    }

    // -- highlighting ---------------------------------------------------------

    private val analyzer = object : SimpleAnalyzeManager<Void>() {
        override fun analyze(text: StringBuilder, delegate: Delegate<Void>): Styles {
            val builder = MappedSpans.Builder(1024)
            val value = text.toString()
            var line = 0
            var lineStart = 0
            var index = 0

            fun span(column: Int, colorId: Int) =
                builder.addIfNeeded(line, column, TextStyle.makeStyle(colorId))

            span(0, EditorColorScheme.TEXT_NORMAL)
            while (index < value.length) {
                if (delegate.isCancelled) break
                val character = value[index]
                when {
                    character == '\n' -> {
                        line++
                        index++
                        lineStart = index
                        span(0, EditorColorScheme.TEXT_NORMAL)
                        continue
                    }
                    // Comments run to end of line; long comments to their close.
                    character == '-' && value.getOrNull(index + 1) == '-' -> {
                        span(index - lineStart, EditorColorScheme.COMMENT)
                        val long = longBracketLength(value, index + 2)
                        val commentEnd = if (long > 0) {
                            skipLongBracket(value, index + 2 + long, long)
                        } else {
                            val newline = value.indexOf('\n', index)
                            if (newline < 0) value.length else newline
                        }
                        // A long comment may span lines, so the line counters
                        // have to be resynced across the skipped region --
                        // otherwise every span below it lands on the wrong row.
                        for (i in index until commentEnd) {
                            if (value[i] == '\n') {
                                line++
                                lineStart = i + 1
                                span(0, EditorColorScheme.COMMENT)
                            }
                        }
                        index = commentEnd
                        span(index - lineStart, EditorColorScheme.TEXT_NORMAL)
                        continue
                    }
                    character == '"' || character == '\'' || character == '`' -> {
                        span(index - lineStart, EditorColorScheme.LITERAL)
                        index++
                        while (index < value.length && value[index] != character) {
                            if (value[index] == '\\' && index + 1 < value.length) {
                                index++
                            } else if (value[index] == '\n') {
                                // Backtick interpolated strings may span lines.
                                line++
                                lineStart = index + 1
                                span(0, EditorColorScheme.LITERAL)
                            }
                            index++
                        }
                        if (index < value.length) index++
                        span(index - lineStart, EditorColorScheme.TEXT_NORMAL)
                        continue
                    }
                    character.isDigit() -> {
                        span(index - lineStart, EditorColorScheme.LITERAL)
                        while (index < value.length &&
                            (value[index].isLetterOrDigit() || value[index] == '.' || value[index] == '_')
                        ) index++
                        span(index - lineStart, EditorColorScheme.TEXT_NORMAL)
                        continue
                    }
                    character.isLetter() || character == '_' -> {
                        val start = index
                        while (index < value.length &&
                            (value[index].isLetterOrDigit() || value[index] == '_')
                        ) index++
                        val word = value.substring(start, index)
                        // A name followed by '(' reads as a call, which is the
                        // cheap way to get function coloring without a parser.
                        var probe = index
                        while (probe < value.length && value[probe] == ' ') probe++
                        val colorId = when {
                            word in KEYWORDS -> EditorColorScheme.KEYWORD
                            word in LITERALS -> EditorColorScheme.LITERAL
                            value.getOrNull(probe) == '(' -> EditorColorScheme.FUNCTION_NAME
                            else -> EditorColorScheme.IDENTIFIER_NAME
                        }
                        span(start - lineStart, colorId)
                        span(index - lineStart, EditorColorScheme.TEXT_NORMAL)
                        continue
                    }
                    else -> index++
                }
            }
            builder.determine(line)
            return Styles(builder.build())
        }
    }

    companion object {
        const val INDENT_WIDTH = 4
        val INDENT_UNIT = " ".repeat(INDENT_WIDTH)

        /**
         * How long the completion worker waits for the Rust index to answer.
         * Sora cancels and re-requests on the next keystroke anyway, so a miss
         * here costs one stale popup frame, not a lost suggestion.
         */
        const val COMPLETION_TIMEOUT_MS = 400L

        private val KEYWORDS = setOf(
            "local", "function", "end", "if", "then", "else", "elseif", "for",
            "while", "repeat", "until", "do", "return", "break", "continue",
            "and", "or", "not", "in", "export", "type", "self"
        )
        private val LITERALS = setOf("true", "false", "nil")

        /** `[[`, `[=[` ... : returns the number of '=' signs, or -1 if absent. */
        private fun longBracketLength(text: String, at: Int): Int {
            if (text.getOrNull(at) != '[') return 0
            var index = at + 1
            while (text.getOrNull(index) == '=') index++
            return if (text.getOrNull(index) == '[') index - at else 0
        }

        private fun skipLongBracket(text: String, from: Int, level: Int): Int {
            val closer = "]" + "=".repeat((level - 1).coerceAtLeast(0)) + "]"
            val end = text.indexOf(closer, from)
            return if (end < 0) text.length else end + closer.length
        }

        /** Strip a trailing `--` comment without cutting inside a string literal. */
        fun stripLuauComment(line: String): String {
            var quote: Char? = null
            var index = 0
            while (index < line.length) {
                val character = line[index]
                if (quote != null) {
                    if (character == '\\') { index += 2; continue }
                    if (character == quote) quote = null
                } else when {
                    character == '"' || character == '\'' || character == '`' -> quote = character
                    character == '-' && index + 1 < line.length && line[index + 1] == '-' ->
                        return line.substring(0, index)
                }
                index++
            }
            return line
        }

        /** Whether a line opens a Luau block, so the next line indents once. */
        fun opensLuauBlock(code: String): Boolean {
            val trimmed = code.trim()
            return trimmed.endsWith("then") || trimmed.endsWith(" do") ||
                trimmed == "do" || trimmed == "repeat" || trimmed == "else" ||
                trimmed.endsWith("{") || trimmed.endsWith("(") || trimmed.endsWith("[") ||
                trimmed.startsWith("else") && trimmed.endsWith("then") ||
                Regex("^(local |const |export )?function\\b").containsMatchIn(trimmed) ||
                Regex("=\\s*function\\s*\\(").containsMatchIn(trimmed)
        }

        /**
         * The keyword closing the block this line opens, or null when no closer
         * is needed (brackets are already paired on type).
         */
        fun closingKeywordFor(code: String): String? {
            val trimmed = code.trim()
            if (trimmed.endsWith("{") || trimmed.endsWith("(") || trimmed.endsWith("[")) return null
            if (trimmed == "else" || trimmed.startsWith("elseif ")) return null
            if (trimmed == "repeat") return "until true"
            val opens = trimmed.endsWith("then") || trimmed.endsWith(" do") || trimmed == "do" ||
                Regex("^(local |const |export )?function\\b").containsMatchIn(trimmed) ||
                Regex("=\\s*function\\s*\\(").containsMatchIn(trimmed)
            return if (opens) "end" else null
        }

        /** Leading whitespace of [line], with tabs expanded to spaces. */
        fun indentOf(line: String): String =
            line.takeWhile { it == ' ' || it == '\t' }.replace("\t", INDENT_UNIT)

        /**
         * Whether the block opened at [line] already has its closer further
         * down, so Enter does not add a duplicate `end` to complete code.
         */
        fun hasMatchingCloser(text: Content, line: Int, closer: String): Boolean {
            val keyword = closer.substringBefore(' ')
            var depth = 1
            for (index in (line + 1) until text.lineCount) {
                val code = stripLuauComment(text.getLineString(index)).trim()
                if (code == keyword || code.startsWith("$keyword ") ||
                    code.startsWith("$keyword)") || code.startsWith("$keyword,")
                ) {
                    depth--
                    if (depth <= 0) return true
                }
                if (opensLuauBlock(code) && closingKeywordFor(code) != null) depth++
            }
            return false
        }

        /**
         * Bracket and quote pairing. Sora inserts the closer and places the
         * caret between the two, replacing the old afterTextChanged handler.
         */
        private val symbolPairs = SymbolPairMatch().apply {
            putPair('(', SymbolPairMatch.SymbolPair("(", ")"))
            putPair('[', SymbolPairMatch.SymbolPair("[", "]"))
            putPair('{', SymbolPairMatch.SymbolPair("{", "}"))
            putPair('"', SymbolPairMatch.SymbolPair("\"", "\""))
            putPair('\'', SymbolPairMatch.SymbolPair("'", "'"))
        }

        /**
         * Smart Enter. Copies the previous line's indent, adds a level after a
         * block opener, and lays down the matching `end` when the block is not
         * already closed -- the behaviour verified for the EditText version.
         */
        private val blockNewlineHandler = object : NewlineHandler {
            override fun matchesRequirement(
                text: Content,
                position: CharPosition,
                style: Styles?
            ): Boolean = true

            override fun handleNewline(
                text: Content,
                position: CharPosition,
                style: Styles?,
                tabSize: Int
            ): NewlineHandleResult {
                val line = text.getLineString(position.line)
                val before = line.substring(0, position.column.coerceIn(0, line.length))
                val indent = indentOf(before)
                val code = stripLuauComment(before).trimEnd()

                if (!opensLuauBlock(code)) {
                    return NewlineHandleResult("\n$indent", 0)
                }
                val inner = indent + INDENT_UNIT
                val closer = closingKeywordFor(code)
                if (closer == null || hasMatchingCloser(text, position.line, closer)) {
                    return NewlineHandleResult("\n$inner", 0)
                }
                // Caret is left on the blank indented line, above the closer.
                val tail = "\n$indent$closer"
                return NewlineHandleResult("\n$inner$tail", tail.length)
            }
        }

        private val newlineHandlers: Array<NewlineHandler> = arrayOf(blockNewlineHandler)

        /** Dark scheme approximating the previous editor's colors. */
        fun darkScheme(): EditorColorScheme = EditorColorScheme().apply {
            setColor(EditorColorScheme.WHOLE_BACKGROUND, android.graphics.Color.rgb(30, 30, 30))
            setColor(EditorColorScheme.LINE_NUMBER_PANEL, android.graphics.Color.rgb(36, 36, 38))
            setColor(EditorColorScheme.LINE_NUMBER_PANEL_TEXT, android.graphics.Color.rgb(110, 110, 118))
            setColor(EditorColorScheme.LINE_NUMBER, android.graphics.Color.rgb(110, 110, 118))
            setColor(EditorColorScheme.TEXT_NORMAL, android.graphics.Color.rgb(225, 225, 225))
            setColor(EditorColorScheme.KEYWORD, android.graphics.Color.rgb(205, 125, 255))
            setColor(EditorColorScheme.COMMENT, android.graphics.Color.rgb(105, 170, 105))
            setColor(EditorColorScheme.LITERAL, android.graphics.Color.rgb(225, 190, 125))
            setColor(EditorColorScheme.IDENTIFIER_NAME, android.graphics.Color.rgb(225, 225, 225))
            setColor(EditorColorScheme.IDENTIFIER_VAR, android.graphics.Color.rgb(225, 225, 225))
            setColor(EditorColorScheme.FUNCTION_NAME, android.graphics.Color.rgb(115, 195, 255))
            setColor(EditorColorScheme.OPERATOR, android.graphics.Color.rgb(225, 225, 225))
            setColor(EditorColorScheme.CURRENT_LINE, android.graphics.Color.rgb(42, 42, 44))
            setColor(EditorColorScheme.SELECTION_INSERT, android.graphics.Color.rgb(120, 190, 255))
            setColor(EditorColorScheme.SELECTION_HANDLE, android.graphics.Color.rgb(120, 190, 255))
            setColor(EditorColorScheme.SELECTED_TEXT_BACKGROUND, android.graphics.Color.rgb(60, 90, 130))
        }

        /** Builds a sora completion item from one Rust suggestion. */
        fun completionItem(label: String, detail: String, insert: String, prefixLength: Int): CompletionItem =
            SimpleCompletionItem(label, detail, prefixLength, insert)
    }
}
