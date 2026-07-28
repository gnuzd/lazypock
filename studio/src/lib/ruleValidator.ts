// ── Rule Syntax Validator ───────────────────────────────
// Client-side validation for PocketBase-compatible filter rules.
// Matches the behavior of the Elixir FilterCompiler in
// core/lib/lazypock/schemas/filter_compiler.ex

export interface RuleValidationResult {
	valid: boolean;
	error?: string;
}

// Operators that produce conditions
const COMPARISON_OPS = new Set(["=", "!=", ">", ">=", "<", "<=", "~", "!~"]);
const LOGICAL_OPS = new Set(["&&", "||"]);

/** Tokenize a rule string into tokens */
function tokenize(rule: string): string[] {
	return rule
		.split(/(&&|\|\||>=|<=|!=|!~|>|<|~|=|!|[()])/)
		.map((t) => t.trim())
		.filter((t) => t !== "");
}

type TokenKind = "literal" | "field" | "op" | "paren" | "not" | "unknown";

function classify(token: string): TokenKind {
	if (COMPARISON_OPS.has(token) || LOGICAL_OPS.has(token)) return "op";
	if (token === "!") return "not";
	if (token === "(" || token === ")") return "paren";

	// Quoted string literal
	if (
		token.startsWith("'") &&
		token.endsWith("'") &&
		token.length >= 2 &&
		!token.slice(1, -1).includes("'")
	) {
		return "literal";
	}

	// Number literal
	if (/^\d+(\.\d+)?$/.test(token)) return "literal";

	// Boolean / null literal
	if (/^(true|false|null)$/i.test(token)) return "literal";

	// Field reference — can include @request.auth.* prefix and dots for relation paths
	if (/^[a-zA-Z_@][a-zA-Z0-9_@.]*$/.test(token)) return "field";

	return "unknown";
}

/** Validate that parentheses are balanced */
function checkBalancedParens(tokens: string[]): string | null {
	let depth = 0;
	for (const t of tokens) {
		if (t === "(") depth++;
		if (t === ")") depth--;
		if (depth < 0) return "Unmatched closing parenthesis";
	}
	if (depth > 0) return "Unclosed opening parenthesis";
	return null;
}

/**
 * Validate the structure of a rule string.
 * Accepts:
 *   - empty/null → valid (public access)
 *   - a single expression
 *   - AND / OR combinations of expressions
 *   - NOT prefix
 *   - parenthesized groups
 *   - field comparisons: field OP value
 *   - literal comparisons: literal OP literal (for @request.auth.* resolved rules)
 */
export function validateRule(rule: string | null | undefined): RuleValidationResult {
	if (rule == null || rule.trim() === "") {
		return { valid: true };
	}

	const tokens = tokenize(rule);

	if (tokens.length === 0) {
		return { valid: true };
	}

	// Check balanced parens first
	const parenError = checkBalancedParens(tokens);
	if (parenError) {
		return { valid: false, error: parenError };
	}

	// Walk tokens in expression mode
	const result = validateExpression(tokens, 0);
	if (!result) {
		return { valid: false, error: "Invalid expression" };
	}

	// If we consumed all tokens, it's fully valid
	if (result.end < tokens.length) {
		return {
			valid: false,
			error: `Unexpected token "${tokens[result.end]}" after expression`,
		};
	}

	return { valid: true };
}

interface ParseResult {
	end: number;
}

/** Parse an expression (OR/AND chain) starting at index i */
function validateExpression(tokens: string[], i: number): ParseResult | null {
	// Parse the first operand
	const first = validateOperand(tokens, i);
	if (!first) return null;

	let pos = first.end;

	// Accept any number of && or || followed by another operand
	while (pos < tokens.length && LOGICAL_OPS.has(tokens[pos])) {
		// Must have something after the operator
		const next = validateOperand(tokens, pos + 1);
		if (!next) {
			return null; // operator with missing right operand
		}
		pos = next.end;
	}

	return { end: pos };
}

/** Parse a single operand (comparison, NOT expression, parenthesized expression) */
function validateOperand(tokens: string[], i: number): ParseResult | null {
	if (i >= tokens.length) return null;
	const t = tokens[i];

	// NOT operator
	if (t === "!") {
		const inner = validateOperand(tokens, i + 1);
		if (!inner) return null;
		return { end: inner.end };
	}

	// Parenthesized sub-expression
	if (t === "(") {
		const inner = validateExpression(tokens, i + 1);
		if (!inner) return null;
		// Must have closing paren
		if (inner.end >= tokens.length || tokens[inner.end] !== ")") {
			return null;
		}
		return { end: inner.end + 1 };
	}

	// Closing paren without opening — handled by paren balance check
	if (t === ")") return null;

	const kind = classify(t);

	// Field or literal — might be a comparison
	if (kind === "field" || kind === "literal") {
		// Look ahead for a comparison operator
		if (i + 1 < tokens.length && COMPARISON_OPS.has(tokens[i + 1])) {
			// Must have a right-hand-side operand
			if (i + 2 >= tokens.length) return null;
			const rhsKind = classify(tokens[i + 2]);
			if (rhsKind === "unknown") return null;
			if (rhsKind === "op" || rhsKind === "not" || rhsKind === "paren") return null;
			return { end: i + 3 };
		}
		// Standalone field (e.g. just a boolean field)
		return { end: i + 1 };
	}

	// Operators are not valid as standalone operands
	if (kind === "op" || kind === "not") return null;
	if (kind === "paren") return null;
	if (kind === "unknown") return null;

	return null;
}
