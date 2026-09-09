package com.yourname.rbxleditor

import android.os.Bundle
import io.github.rosemoe.sora.lang.EmptyLanguage
import io.github.rosemoe.sora.lang.analysis.AnalyzeManager
import io.github.rosemoe.sora.lang.analysis.SimpleAnalyzeManager
import io.github.rosemoe.sora.lang.completion.CompletionCancelledException
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

    /**
     * Bracket and quote pairing, replacing the old afterTextChanged handler.
     *
     * Per instance, never shared: setEditorLanguage() calls setParent() on
     * whatever this returns, so a single static SymbolPairMatch handed to two
     * editors gets its parent reassigned underneath the first one.
     */
    private val symbolPairs = SymbolPairMatch().apply {
        putPair('(', SymbolPairMatch.SymbolPair("(", ")"))
        putPair('[', SymbolPairMatch.SymbolPair("[", "]"))
        putPair('{', SymbolPairMatch.SymbolPair("{", "}"))
        putPair('"', SymbolPairMatch.SymbolPair("\"", "\""))
        putPair('\'', SymbolPairMatch.SymbolPair("'", "'"))
    }

    private val newlineHandlers: Array<NewlineHandler> = arrayOf(blockNewlineHandler)

    override fun getSymbolPairs(): SymbolPairMatch = symbolPairs

    override fun getNewlineHandlers(): Array<NewlineHandler> = newlineHandlers

    override fun requireAutoComplete(
        content: ContentReference,
        position: CharPosition,
        publisher: CompletionPublisher,
        extraArguments: Bundle
    ) {
        try {
            publishCompletions(content, position, publisher)
        } catch (_: CompletionCancelledException) {
            // Sora's own signal that this request was superseded; not an error.
        } catch (error: Throwable) {
            // Also a worker thread: never let a completion failure take the
            // process down, just offer nothing this keystroke.
            android.util.Log.e("rbxl_editor", "Luau completion failed", error)
        }
    }

    private fun publishCompletions(
        content: ContentReference,
        position: CharPosition,
        publisher: CompletionPublisher
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
        /**
         * Runs on sora's analyzer thread. Any throw here happens off the UI
         * thread, where the caller's fallback cannot catch it, and would kill
         * the process. Returning unstyled text is always better than that, so
         * failures degrade to plain black-and-white rather than crashing.
         */
        override fun analyze(text: StringBuilder, delegate: Delegate<Void>): Styles =
            try {
                highlight(text, delegate)
            } catch (error: Throwable) {
                android.util.Log.e("rbxl_editor", "Luau highlighting failed", error)
                Styles(MappedSpans.Builder(1).also { it.determine(0) }.build())
            }

        private fun highlight(text: StringBuilder, delegate: Delegate<Void>): Styles {
            val builder = MappedSpans.Builder(1024)
            val value = text.toString()
            var line = 0
            var lineStart = 0
            // Last significant character, used to tell `a.b` / `a:b` members
            // from free variables.
            var previousSignificant = ' '

            /** Emits a span, advancing the row counters across any newlines. */
            fun spanTo(index: Int, colorId: Int) {
                builder.addIfNeeded(line, (index - lineStart).coerceAtLeast(0), TextStyle.makeStyle(colorId))
            }

            /**
             * Moves the cursor to [target], keeping line/column in sync. Tokens
             * may span lines (long strings and comments), and MappedSpans
             * throws if spans arrive on a decreasing line, so the counters have
             * to be advanced across every newline that is skipped over.
             */
            fun advance(from: Int, target: Int, colorId: Int) {
                for (i in from until target) {
                    if (value[i] == '\n') {
                        line++
                        lineStart = i + 1
                        builder.addIfNeeded(line, 0, TextStyle.makeStyle(colorId))
                    }
                }
            }

            var index = 0
            spanTo(0, EditorColorScheme.TEXT_NORMAL)
            while (index < value.length) {
                if (delegate.isCancelled) break
                val character = value[index]

                if (character == '\n') {
                    line++
                    index++
                    lineStart = index
                    spanTo(index, EditorColorScheme.TEXT_NORMAL)
                    continue
                }
                if (character == ' ' || character == '\t' || character == '\r') {
                    index++
                    continue
                }

                // Comments: `--` to end of line, or --[[ ]] / --[==[ ]==].
                if (character == '-' && value.getOrNull(index + 1) == '-') {
                    val level = longBracketLength(value, index + 2)
                    val end = if (level > 0) {
                        skipLongBracket(value, index + 2 + level, level)
                    } else {
                        val newline = value.indexOf('\n', index)
                        if (newline < 0) value.length else newline
                    }
                    spanTo(index, EditorColorScheme.COMMENT)
                    advance(index, end, EditorColorScheme.COMMENT)
                    index = end
                    spanTo(index, EditorColorScheme.TEXT_NORMAL)
                    continue
                }

                // Long strings: [[ ]] and [==[ ]==].
                val openLevel = longBracketLength(value, index)
                if (openLevel > 0) {
                    val end = skipLongBracket(value, index + openLevel, openLevel)
                    spanTo(index, EditorColorScheme.LITERAL)
                    advance(index, end, EditorColorScheme.LITERAL)
                    index = end
                    spanTo(index, EditorColorScheme.TEXT_NORMAL)
                    continue
                }

                // Interpolated strings: the {...} holes contain real code, so
                // they are highlighted as code rather than as string body.
                if (character == '`') {
                    spanTo(index, EditorColorScheme.LITERAL)
                    var scan = index + 1
                    while (scan < value.length && value[scan] != '`') {
                        val c = value[scan]
                        if (c == '\\') {
                            if (value.getOrNull(scan + 1) == '\n') {
                                line++
                                lineStart = scan + 2
                                builder.addIfNeeded(line, 0, TextStyle.makeStyle(EditorColorScheme.LITERAL))
                            }
                            scan += 2
                            continue
                        }
                        if (c == '\n') {
                            line++
                            lineStart = scan + 1
                            builder.addIfNeeded(line, 0, TextStyle.makeStyle(EditorColorScheme.LITERAL))
                            scan++
                            continue
                        }
                        if (c == '{') {
                            spanTo(scan, EditorColorScheme.OPERATOR)
                            spanTo(scan + 1, EditorColorScheme.TEXT_NORMAL)
                            var depth = 1
                            var k = scan + 1
                            while (k < value.length && depth > 0) {
                                when (value[k]) {
                                    '{' -> depth++
                                    '}' -> depth--
                                    '\\' -> {
                                        if (value.getOrNull(k + 1) == '\n') {
                                            line++
                                            lineStart = k + 2
                                            builder.addIfNeeded(line, 0, TextStyle.makeStyle(EditorColorScheme.TEXT_NORMAL))
                                        }
                                        k++
                                    }
                                    '\n' -> {
                                        line++
                                        lineStart = k + 1
                                        builder.addIfNeeded(line, 0, TextStyle.makeStyle(EditorColorScheme.TEXT_NORMAL))
                                    }
                                }
                                k++
                            }
                            if (depth == 0) {
                                spanTo(k - 1, EditorColorScheme.OPERATOR)
                                spanTo(k, EditorColorScheme.LITERAL)
                            }
                            scan = k
                            continue
                        }
                        scan++
                    }
                    val end = (scan + 1).coerceAtMost(value.length)
                    index = end
                    spanTo(index, EditorColorScheme.TEXT_NORMAL)
                    continue
                }

                // Quoted strings; a bare newline ends them, as in Luau.
                if (character == '"' || character == '\'') {
                    spanTo(index, EditorColorScheme.LITERAL)
                    var scan = index + 1
                    while (scan < value.length && value[scan] != character) {
                        if (value[scan] == '\\') {
                            // A backslash-escaped newline stays inside the
                            // string, so the row counter has to follow it.
                            if (value.getOrNull(scan + 1) == '\n') {
                                line++
                                lineStart = scan + 2
                                builder.addIfNeeded(line, 0, TextStyle.makeStyle(EditorColorScheme.LITERAL))
                                scan += 2
                                continue
                            }
                            scan++
                        }
                        if (scan < value.length && value[scan] == '\n') break
                        scan++
                    }
                    // An unterminated string stops AT the newline. Consuming it
                    // here would leave `line` behind the real text and produce
                    // a column past the end of the row.
                    index = if (scan < value.length && value[scan] == '\n') {
                        scan
                    } else {
                        (scan + 1).coerceAtMost(value.length)
                    }
                    spanTo(index, EditorColorScheme.TEXT_NORMAL)
                    continue
                }

                // Numbers: 0xFF_A0, 0b1010, 1_000.5e-3, .5
                if (character.isDigit() ||
                    (character == '.' && value.getOrNull(index + 1)?.isDigit() == true)
                ) {
                    var scan = index
                    val second = value.getOrNull(index + 1)
                    if (character == '0' && (second == 'x' || second == 'X')) {
                        scan = index + 2
                        while (scan < value.length &&
                            (value[scan].isDigit() || value[scan] in "abcdefABCDEF_")
                        ) scan++
                    } else if (character == '0' && (second == 'b' || second == 'B')) {
                        scan = index + 2
                        while (scan < value.length && (value[scan] == '0' || value[scan] == '1' || value[scan] == '_')) scan++
                    } else {
                        while (scan < value.length && (value[scan].isDigit() || value[scan] == '_')) scan++
                        if (scan < value.length && value[scan] == '.') {
                            scan++
                            while (scan < value.length && (value[scan].isDigit() || value[scan] == '_')) scan++
                        }
                        if (scan < value.length && (value[scan] == 'e' || value[scan] == 'E')) {
                            var exponent = scan + 1
                            if (exponent < value.length && (value[exponent] == '+' || value[exponent] == '-')) exponent++
                            if (exponent < value.length && value[exponent].isDigit()) {
                                scan = exponent
                                while (scan < value.length && value[scan].isDigit()) scan++
                            }
                        }
                    }
                    spanTo(index, EditorColorScheme.LITERAL)
                    index = scan
                    spanTo(index, EditorColorScheme.TEXT_NORMAL)
                    continue
                }

                // Attributes: @native, @checked.
                if (character == '@') {
                    var scan = index + 1
                    while (scan < value.length && (value[scan].isLetterOrDigit() || value[scan] == '_')) scan++
                    spanTo(index, EditorColorScheme.ANNOTATION)
                    index = scan
                    spanTo(index, EditorColorScheme.TEXT_NORMAL)
                    continue
                }

                // Identifiers and keywords.
                if (character.isLetter() || character == '_') {
                    var scan = index
                    while (scan < value.length && (value[scan].isLetterOrDigit() || value[scan] == '_')) scan++
                    val word = value.substring(index, scan)
                    var probe = scan
                    while (probe < value.length && (value[probe] == ' ' || value[probe] == '\t')) probe++
                    val next = value.getOrNull(probe)
                    val colorId = when {
                        word in KEYWORDS -> EditorColorScheme.KEYWORD
                        word in LITERALS -> EditorColorScheme.LITERAL
                        word in BUILTIN_TYPES -> EditorColorScheme.KEYWORD
                        // A name applied to a call, string or table literal is
                        // a call: f(), f"s", f{...}, f`s`.
                        next == '(' || next == '`' || next == '{' || next == '"' || next == '\'' ->
                            EditorColorScheme.FUNCTION_NAME
                        previousSignificant == ':' || previousSignificant == '.' ->
                            EditorColorScheme.IDENTIFIER_NAME
                        else -> EditorColorScheme.IDENTIFIER_VAR
                    }
                    spanTo(index, colorId)
                    index = scan
                    spanTo(index, EditorColorScheme.TEXT_NORMAL)
                    previousSignificant = 'w'
                    continue
                }

                // Operators and punctuation, longest match first so that `::`,
                // `->`, `+=` and `...` are single tokens.
                if (character in OPERATOR_CHARS) {
                    val three = value.substring(index, (index + 3).coerceAtMost(value.length))
                    val two = value.substring(index, (index + 2).coerceAtMost(value.length))
                    val width = when {
                        three in OPERATORS_3 -> 3
                        two in OPERATORS_2 -> 2
                        else -> 1
                    }
                    spanTo(index, EditorColorScheme.OPERATOR)
                    previousSignificant = if (width == 1) character else 'o'
                    index += width
                    spanTo(index, EditorColorScheme.TEXT_NORMAL)
                    continue
                }

                index++
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

        /**
         * Builtin type names. Coloured as keywords wherever they appear:
         * they are not reserved words, but shadowing them is rare enough that
         * treating them as types reads better than treating them as variables.
         */
        private val BUILTIN_TYPES = setOf(
            "number", "string", "boolean", "thread", "userdata", "any",
            "unknown", "never", "buffer", "vector", "typeof", "keyof"
        )

        private const val OPERATOR_CHARS = "+-*/%^#=~<>(){}[];:,.|&?"

        /** Three-character operators, matched before the two-character set. */
        private val OPERATORS_3 = setOf("...", "//=", "..=")

        private val OPERATORS_2 = setOf(
            "==", "~=", "<=", ">=", "..", "->", "+=", "-=", "*=", "/=",
            "%=", "^=", "::", "//"
        )

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
            // Members after '.' or ':' get their own tint so `obj.field` reads
            // differently from a free variable.
            setColor(EditorColorScheme.IDENTIFIER_NAME, android.graphics.Color.rgb(156, 220, 254))
            setColor(EditorColorScheme.IDENTIFIER_VAR, android.graphics.Color.rgb(225, 225, 225))
            setColor(EditorColorScheme.FUNCTION_NAME, android.graphics.Color.rgb(115, 195, 255))
            // Dimmer than text so punctuation recedes and names stand out.
            setColor(EditorColorScheme.OPERATOR, android.graphics.Color.rgb(190, 190, 195))
            // Attributes such as @native / @checked.
            setColor(EditorColorScheme.ANNOTATION, android.graphics.Color.rgb(220, 220, 145))
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
