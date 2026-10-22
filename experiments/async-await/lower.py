#!/usr/bin/env python3
"""Compile a deliberately tiny async syntax to movable, owned C state machines.

This is an isolated design experiment, not a Plenty frontend or a Python runner.
Only the forms checked below exist. Host/compiler allocation is outside the
generated program's allocation contract.
"""

import ast
from pathlib import Path
import re
import sys


class Unsupported(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise Unsupported(message)


def identifier(name):
    require(re.fullmatch(r"[a-z][a-z0-9_]*", name), "use lowercase identifiers")
    require(name not in {"pc", "tick", "acquire", "fail"}, "reserved identifier")
    return "v_" + name


def lower(source):
    tree = ast.parse(source)
    signatures = {}
    output = ['#include "runtime.h"']
    for fn in tree.body:
        require(isinstance(fn, ast.AsyncFunctionDef), "only async def is supported")
        name = identifier(fn.name)
        require(fn.name not in signatures, "duplicate function")
        args = fn.args
        require(not (args.posonlyargs or args.vararg or args.kwarg or
                     args.kwonlyargs or args.defaults or fn.decorator_list),
                "only positional i64 parameters, without defaults/decorators")
        require(isinstance(fn.returns, ast.Name) and fn.returns.id == "i64",
                "result annotation must be i64")
        parameters = []
        locals_ = {}
        for arg in args.args:
            identifier(arg.arg)
            require(arg.arg not in locals_, "duplicate parameter")
            require(isinstance(arg.annotation, ast.Name) and arg.annotation.id == "i64",
                    "parameters must be owned i64; borrowed parameters unsupported")
            locals_[arg.arg] = "int"
            parameters.append(arg.arg)
        fields = ["int pc;"] + [f"int64_t {identifier(p)};" for p in parameters]
        cleanup = []
        cases = []

        def expr(node):
            if isinstance(node, ast.Constant) and type(node.value) is int:
                require(0 <= node.value <= 2**31 - 1, "literal outside spike range")
                return str(node.value)
            if isinstance(node, ast.Name):
                require(locals_.get(node.id) == "int", "expression needs initialized i64")
                return "f->" + identifier(node.id)
            if isinstance(node, ast.BinOp) and isinstance(node.op, (ast.Add, ast.Sub)):
                operator = "add_i64" if isinstance(node.op, ast.Add) else "sub_i64"
                return f"{operator}({expr(node.left)}, {expr(node.right)})"
            raise Unsupported("only i64 literals, bindings, addition/subtraction in expressions")

        require(fn.body and isinstance(fn.body[-1], ast.Return), "final return required")
        for index, stmt in enumerate(fn.body):
            # Each await has an entry state (initialization) and a resume state.
            pc = index * 2
            lines = [f"case {pc}: {{"]
            target = None
            value = None
            if isinstance(stmt, ast.Assign):
                require(len(stmt.targets) == 1 and isinstance(stmt.targets[0], ast.Name),
                        "assignment needs one fresh binding")
                target = stmt.targets[0].id
                identifier(target)
                require(target not in locals_, "bindings cannot be reassigned in the spike")
                value = stmt.value
            elif isinstance(stmt, ast.Expr):
                value = stmt.value
            elif isinstance(stmt, ast.Return):
                require(index == len(fn.body) - 1, "only a final return is supported")
                lines += [f"Outcome result = ok({expr(stmt.value)});",
                          f"{name}_drop(f);", "return result;"]
            else:
                raise Unsupported("unsupported statement (loops, borrows, with and try excluded)")

            if isinstance(value, ast.Await):
                call = value.value
                require(isinstance(call, ast.Call) and isinstance(call.func, ast.Name)
                        and not call.keywords, "await needs a direct positional call")
                callee = call.func.id
                require(callee == "tick" or callee in signatures,
                        "await target must be defined earlier; recursion/forward calls excluded")
                expected = 1 if callee == "tick" else signatures[callee]
                require(len(call.args) == expected, "wrong await argument count")
                arguments = ", ".join(expr(a) for a in call.args)
                child = f"child_{index}"
                ctype = "Tick" if callee == "tick" else identifier(callee)
                prefix = "tick" if callee == "tick" else identifier(callee)
                fields += [f"{ctype} {child};", f"bool live_{index};"]
                cleanup.insert(0, f"if (f->live_{index}) {{ {prefix}_drop(&f->{child}); "
                                  f"f->live_{index} = false; }}")
                lines += [f"f->{child} = {prefix}_new({arguments});",
                          f"f->live_{index} = true;", f"f->pc = {pc + 1};",
                          "} /* fall through */", f"case {pc + 1}: {{",
                          f"Outcome result = {prefix}_poll(&f->{child});",
                          "if (result.kind == PENDING) return result;",
                          f"f->live_{index} = false;",
                          f"if (result.kind == ERROR) {{ {name}_drop(f); return result; }}"]
                if target is not None:
                    locals_[target] = "int"
                    fields.append(f"int64_t {identifier(target)};")
                    lines.append(f"f->{identifier(target)} = result.value;")
            elif isinstance(value, ast.Call) and isinstance(value.func, ast.Name):
                require(not value.keywords, "keyword arguments unsupported")
                if value.func.id == "acquire":
                    require(target is not None and not value.args,
                            "acquire() must initialize a fresh resource binding")
                    locals_[target] = "resource"
                    field = identifier(target)
                    fields.append(f"void *{field};")
                    cleanup.insert(0, f"release_resource(&f->{field});")
                    lines += [f"f->{field} = acquire();",
                              f"if (!f->{field}) {{ {name}_drop(f); return error(ALLOC); }}"]
                elif value.func.id == "fail":
                    require(target is None and len(value.args) == 1,
                            "fail(code) is a statement")
                    lines += [f"Outcome result = error({expr(value.args[0])});",
                              f"{name}_drop(f);", "return result;"]
                else:
                    raise Unsupported("ordinary calls unsupported")
            elif value is not None:
                require(target is not None, "bare expression unsupported")
                expression = expr(value)
                locals_[target] = "int"
                fields.append(f"int64_t {identifier(target)};")
                lines.append(f"f->{identifier(target)} = {expression};")
            if not isinstance(stmt, ast.Return):
                lines += [f"f->pc = {pc + 2};", "} /* fall through */"]
            else:
                lines += ["}"]
            cases.extend(lines)

        params = ", ".join(f"int64_t {identifier(p)}" for p in parameters) or "void"
        assigns = " ".join(f"f.{identifier(p)} = {identifier(p)};" for p in parameters)
        output += [f"typedef struct {{ {' '.join(fields)} }} {name};",
                   f"static {name} {name}_new({params}) {{ {name} f = {{0}}; {assigns} return f; }}",
                   f"static void {name}_drop({name} *f) {{ {' '.join(cleanup)} f->pc = -1; }}",
                   f"static Outcome {name}_poll({name} *f) {{ switch (f->pc) {{",
                   *cases, "default: return error(FINISHED); } }",
                   f"static Outcome {name}_erased_poll(void *f) {{ return {name}_poll(f); }}",
                   f"static void {name}_erased_drop(void *f) {{ {name}_drop(f); }}"]
        signatures[fn.name] = len(parameters)
    require(signatures, "at least one async function required")
    return "\n".join(output) + "\n"


if __name__ == "__main__":
    try:
        print(lower(Path(sys.argv[1]).read_text()), end="")
    except (Unsupported, SyntaxError) as error:
        sys.exit(f"unsupported spike input: {error}")
