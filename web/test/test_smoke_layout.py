"""The browser checks stay one module per page (web/test/page_<name>.py), so that two changes to two pages never
edit the same file. Runs without a browser."""
import ast
import pathlib
import re
import unittest

HERE = pathlib.Path(__file__).resolve().parent
# What browser.py may grow to before it is again the file every change edits: a helper only one page uses belongs in
# that page's module, and one that several pages use but that is about one area may get a module of its own.
SHARED_LINES = 400


class PageLayout(unittest.TestCase):
    def test_smoke_py_holds_no_check(self):
        module = ast.parse((HERE / "smoke.py").read_text())
        functions = sorted(node.name for node in module.body if isinstance(node, ast.FunctionDef))
        self.assertEqual(functions, ["main", "pages"],
                         "smoke.py is the dispatcher only: a page's check and its helpers go in web/test/page_<name>.py, "
                         "shared helpers in web/test/browser.py")

    def test_every_page_module_names_its_own_flag_first(self):
        for path in sorted(HERE.glob("page_*.py")):
            with self.subTest(page=path.name):
                text = path.read_text()
                flag = "--" + path.stem.removeprefix("page_").replace("_", "-") + "-page"
                self.assertIn(f'FLAG = "{flag}"', text, f"{path.name} must declare FLAG = {flag!r} (the file is named by its flag)")
                self.assertRegex(text, r"(?m)^ARGUMENTS = \(\n    \(\d+, \('" + re.escape(flag) + r"'",
                                 f"{path.name}: the first of ARGUMENTS is the page's own flag")
                self.assertRegex(text, r"(?m)^ORDER = \d+$", f"{path.name} must say when it runs (ORDER)")
                self.assertRegex(text, r"(?m)^def run\(args\):", f"{path.name} must define run(args)")

    def test_the_shared_module_stays_small(self):
        lines = len((HERE / "browser.py").read_text().splitlines())
        self.assertLessEqual(lines, SHARED_LINES, f"web/test/browser.py has {lines} lines (at most {SHARED_LINES}): move what "
                             "one page uses into that page's module, or give an area its own module")


if __name__ == "__main__":
    unittest.main()
