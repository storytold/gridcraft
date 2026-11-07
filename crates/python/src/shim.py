# GridCraft's Python worker, run by the user's own Python (3.11 or newer).
#
# GridCraft starts it as `python -I -S -E shim.py <package folders...>` with an empty environment,
# from a private copy of this file, confined by the sandbox launcher. It speaks the JSON-lines protocol in crates/python/src/protocol.rs: requests arrive
# one per line on standard input; each gets exactly one reply line on standard output. This file
# is data that the user's interpreter runs; it is the one file in the repository that is not Rust.
#
# Everything lives inside one function, whose name is removed from the module before it runs, so
# the cells' code finds no helpers in `__main__`, and the protocol's pipes are held in closures.
# That keeps honest code away from the protocol; it can't stop code that goes looking, which is
# why GridCraft treats every line from here as untrusted.


def _gridcraft_worker():
    import ast
    import base64
    import builtins
    import datetime
    import errno
    import importlib
    import io
    import json
    import math
    import os
    import sys
    import sysconfig
    import types

    globals().pop("_gridcraft_worker", None)

    # The caps of protocol.rs. GridCraft kills the worker for a line that breaks one, so the shim
    # keeps its replies within them.
    MAX_LINE_BYTES = 67108864
    MAX_TEXT_BYTES = 131072
    MAX_MESSAGE_BYTES = 8192
    MAX_SUBJECT_BYTES = 1024
    MAX_TYPE_NAME_BYTES = 256
    MAX_DATE_BYTES = 64
    MAX_PREVIEW_CHARS = 255
    MAX_SIZE_DIMS = 8
    MAX_SIZE = 9007199254740992
    MAX_TABLE_ROWS = 200
    MAX_TABLE_COLS = 30
    MAX_TABLE_TEXT_BYTES = 4096
    MAX_TABLE_BYTES = 1048576
    MAX_STDOUT_BYTES = 1048576
    MAX_ARRAY_CELLS = 1000000
    MAX_PICTURE_BYTES = 16777216
    MAX_PICTURE_SIDE = 8192
    MAX_PICTURE_PIXELS = 16000000

    ERRORS = frozenset(
        ("#NULL!", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A", "#GETTING_DATA", "#SPILL!",
         "#CALC!", "#PYTHON!", "#BUSY!", "#TIMEOUT!", "#BLOCKED!", "#CONNECT!")
    )

    # Installed packages, without `site` (which would run .pth files): GridCraft worked out the
    # package folders and hands them over, since this process may not even know it belongs to a
    # virtual environment. Without any, the interpreter's own folders are used.
    folders = sys.argv[1:]
    if not folders:
        try:
            paths = sysconfig.get_paths()
        except Exception:
            paths = {}
        folders = [paths.get(key) for key in ("purelib", "platlib")]
    for p in folders:
        if p and p not in sys.path and os.path.isdir(p):
            sys.path.append(p)

    # The sandbox lets nothing be written, so bytecode caches aren't tried. Numerical libraries stay
    # on one thread.
    sys.dont_write_bytecode = True
    os.environ["OPENBLAS_NUM_THREADS"] = "1"

    # The protocol's pipes are private copies. Standard input becomes an empty pipe, so input()
    # can't read requests, and standard output goes to standard error, so text that C code or a
    # child process prints can't break a line.
    proto_in = os.fdopen(os.dup(0), "rb")
    proto_out = os.fdopen(os.dup(1), "wb")
    r, w = os.pipe()
    os.close(w)
    os.dup2(r, 0)
    os.close(r)
    os.dup2(2, 1)
    sys.stdin = io.StringIO("")
    encode = json.JSONEncoder(ensure_ascii=False, allow_nan=False, separators=(",", ":")).encode
    decode = json.JSONDecoder().decode
    try:
        import signal

        signal.signal(signal.SIGINT, signal.SIG_IGN)
    except Exception:
        pass

    def send(msg):
        try:
            data = encode(msg).encode("utf-8", "replace")
        except Exception:
            data = None
        if data is None or len(data) > MAX_LINE_BYTES:
            data = encode(done_error(msg.get("id"), "The result is too large to send to GridCraft", None)).encode("utf-8")
        proto_out.write(data + b"\n")
        proto_out.flush()

    def done_error(rid, message, line, stdout="", kind=None, subject=None):
        err = {"message": cut_bytes(message, MAX_MESSAGE_BYTES)}
        if isinstance(line, int) and not isinstance(line, bool) and 0 <= line <= 4294967295:
            err["line"] = line
        if kind:
            err["kind"] = kind
        if isinstance(subject, str) and subject:
            err["subject"] = cut_bytes(subject, MAX_SUBJECT_BYTES)
        msg = {"type": "done", "id": rid, "ok": False, "error": err}
        if stdout:
            msg["stdout"] = stdout
        return msg

    def cut_bytes(s, n):
        b = s.encode("utf-8", "replace")
        return s if len(b) <= n else b[:n].decode("utf-8", "ignore")

    def cut_chars(s, n):
        return s if len(s) <= n else s[: n - 3] + "..."

    class TooBig(Exception):
        pass

    # Excel's error values, as Python sees them: xl() gives them for error cells, and a result
    # that holds one becomes that error again.
    class ExcelError:
        __slots__ = ("text",)

        def __init__(self, text):
            self.text = text

        def __repr__(self):
            return self.text

        __str__ = __repr__

        def __eq__(self, other):
            return isinstance(other, ExcelError) and other.text == self.text

        def __hash__(self):
            return hash(self.text)

    ExcelError.__module__ = "gridcraft"

    # Inputs: wire values to Python.
    def to_py(v):
        if isinstance(v, float):
            return int(v) if v.is_integer() and abs(v) < 2**53 else v
        if v is None or isinstance(v, (bool, int, str)):
            return v
        if isinstance(v, dict):
            if "error" in v:
                return ExcelError(str(v["error"]))
            a = v.get("array")
            if isinstance(a, dict):
                rows, cols, data = a.get("rows", 0), a.get("cols", 0), a.get("data", [])
                return [[to_py(x) for x in data[i * cols:(i + 1) * cols]] for i in range(rows)]
            return date_to_py(v)
        return None

    def date_to_py(v):
        # GridCraft works out dates from serial numbers, in the workbook's date system. A serial that
        # names no day never gets here: it arrives as a number.
        try:
            if "date" in v:
                return datetime.date.fromisoformat(v["date"])
            if "dateTime" in v:
                return datetime.datetime.fromisoformat(v["dateTime"])
            if "time" in v:
                return datetime.time.fromisoformat(v["time"])
        except (TypeError, ValueError):
            return ExcelError("#VALUE!")
        return None

    state = {"inputs": None}

    class XlReference(ValueError):
        pass

    XlReference.__name__ = "ValueError"
    XlReference.__qualname__ = "ValueError"

    # The names Excel's default initialization imports, so code written for it runs here. Each is
    # imported the first time it is used, so a package that isn't installed only matters to the code
    # that uses it, and starting Python stays quick.
    class Lazy:
        __slots__ = ("_module",)

        def __init__(self, module):
            object.__setattr__(self, "_module", module)

        def __getattr__(self, name):
            return getattr(importlib.import_module(object.__getattribute__(self, "_module")), name)

        def __repr__(self):
            return repr(importlib.import_module(object.__getattribute__(self, "_module")))

    DEFAULT_IMPORTS = {"np": "numpy", "pd": "pandas", "sm": "statsmodels.api", "plt": "matplotlib.pyplot", "sns": "seaborn"}

    # matplotlib, as it is first imported. It draws without a window, and it insists on a folder for
    # its settings and font list. The sandbox gives it nothing to write to, so that one call gets a
    # folder that doesn't exist: matplotlib can't save its font list there, keeps the list in memory
    # and carries on, and the worker still writes nothing.
    def before_matplotlib():
        import logging
        import tempfile
        import warnings

        os.environ["MPLBACKEND"] = "agg"
        logging.getLogger("matplotlib").setLevel(logging.ERROR)
        warnings.filterwarnings("ignore", message=".*is non-interactive, and thus cannot be shown")
        mkdtemp = tempfile.mkdtemp

        def only_for_others(suffix=None, prefix=None, dir=None):
            if prefix == "matplotlib-":
                return "/nonexistent-gridcraft-matplotlib"
            return mkdtemp(suffix, prefix, dir)

        tempfile.mkdtemp = only_for_others

    # Fonts that cover the scripts matplotlib's own font doesn't, in the order they are tried. A few
    # are enough, and each one costs time at a plot's first text.
    NON_LATIN_FONTS = (
        "Arial Unicode MS",
        "Noto Sans CJK SC",
        "Noto Sans CJK JP",
        "Hiragino Sans GB",
        "Apple SD Gothic Neo",
        "Noto Sans Thai",
        "Thonburi",
        "WenQuanYi Zen Hei",
        "Droid Sans Fallback",
        "Microsoft YaHei",
        "Malgun Gothic",
        "Leelawadee UI",
    )

    def use_non_latin_fonts():
        # Text in any script draws without the code choosing a font: matplotlib falls back along the
        # font families, and only a list of families gives it anywhere to fall back to.
        matplotlib = sys.modules["matplotlib"]
        fonts = sys.modules["matplotlib.font_manager"].fontManager.ttflist
        have = {f.name for f in fonts}
        extra = [name for name in NON_LATIN_FONTS if name in have][:3]
        if extra and "DejaVu Sans" in have and list(matplotlib.rcParams["font.family"]) == ["sans-serif"]:
            matplotlib.rcParams["font.family"] = ["DejaVu Sans"] + extra

    # The file of each kind of font Excel's excel.FontPath names, so code written for it runs here.
    EXCEL_FONTS = {
        "CHINESE_SIMPLIFIED": ("Noto Sans CJK SC", "Hiragino Sans GB", "Microsoft YaHei", "WenQuanYi Zen Hei", "Songti SC", "Arial Unicode MS"),
        "CHINESE_TRADITIONAL": ("Noto Sans CJK TC", "Heiti TC", "Microsoft JhengHei", "Arial Unicode MS"),
        "JAPANESE": ("Noto Sans CJK JP", "Hiragino Sans", "Yu Gothic", "Arial Unicode MS"),
        "KOREAN": ("Noto Sans CJK KR", "Apple SD Gothic Neo", "AppleGothic", "Malgun Gothic", "Arial Unicode MS"),
        "THAI": ("Noto Sans Thai", "Thonburi", "Leelawadee UI", "Arial Unicode MS"),
    }

    class FontPaths:
        def __getattr__(self, name):
            names = EXCEL_FONTS.get(name)
            if names is None:
                raise AttributeError("excel.FontPath has no %s" % name)
            manager = importlib.import_module("matplotlib.font_manager").fontManager
            for wanted in names:
                for font in manager.ttflist:
                    if font.name == wanted:
                        return font.fname
            raise AttributeError("no font for %s on this machine" % name)

    excel = types.ModuleType("excel")
    excel.FontPath = FontPaths()
    sys.modules["excel"] = excel

    class PlotHook:
        # A finder that finds nothing of its own: the import system asks it first, which is the
        # moment to act.
        @staticmethod
        def find_spec(name, path=None, target=None):
            if name == "matplotlib":
                try:
                    before_matplotlib()
                except Exception:
                    pass
            elif name == "matplotlib.figure":
                return figure_module_spec(name, path, target)
            return None

    def figure_module_spec(name, path, target):
        machinery = importlib.import_module("importlib.machinery")
        spec = machinery.PathFinder.find_spec(name, path, target)
        loader = getattr(spec, "loader", None)
        if loader is not None:
            run_module = loader.exec_module

            def exec_module(module):
                run_module(module)
                for step in (use_non_latin_fonts, lambda: watch_sizes(module)):
                    try:
                        step()
                    except Exception:
                        pass

            loader.exec_module = exec_module
        return spec

    # Whether the code chose a figure's size. A figure whose size it didn't choose is drawn again when
    # its picture is resized. These watch the calls rather than read the code, so helper functions,
    # **kwargs and earlier cells are all seen. seaborn computes sizes of its own, which aren't a
    # choice: its functions are compared with their defaults instead.
    SEABORN_SIZES = {
        "pairplot": {"height": 2.5, "aspect": 1},
        "PairGrid": {"height": 2.5, "aspect": 1},
        "FacetGrid": {"height": 3, "aspect": 1},
        "catplot": {"height": 5, "aspect": 1},
        "relplot": {"height": 5, "aspect": 1},
        "displot": {"height": 5, "aspect": 1},
        "lmplot": {"height": 5, "aspect": 1},
        "jointplot": {"height": 6},
        "JointGrid": {"height": 6},
        "clustermap": {"figsize": (10, 10)},
    }

    def in_folder(frame, name):
        return (os.sep + name + os.sep) in frame.f_code.co_filename

    def seaborn_size_chosen(frame):
        """Whether the outermost seaborn function on the stack was given a size of its own. None when
        no seaborn function is on the stack."""
        outer = None
        while frame is not None:
            if in_folder(frame, "seaborn"):
                outer = frame
            frame = frame.f_back
        if outer is None:
            return None
        name = outer.f_code.co_name
        if name == "__init__":
            name = type(outer.f_locals.get("self")).__name__
        for key, default in SEABORN_SIZES.get(name, {}).items():
            value = outer.f_locals.get(key, default)
            if value is not None and tuple(numbers(value)) != tuple(numbers(default)):
                return True
        return False

    def numbers(value):
        return (float(x) for x in (value if isinstance(value, (list, tuple)) else (value,)))

    def watch_sizes(module):
        functools = importlib.import_module("functools")
        figure = module.Figure
        init = figure.__init__
        set_size = figure.set_size_inches

        # Watching never gets in the way of a plot: anything it can't work out counts as a choice.
        @functools.wraps(init)
        def watched_init(self, *args, **kwargs):
            try:
                chosen = seaborn_size_chosen(sys._getframe(1))
            except Exception:
                chosen = True
            if chosen is True or (chosen is None and kwargs.get("figsize", args[0] if args else None) is not None):
                self._gridcraft_fixed = True
            init(self, *args, **kwargs)

        @functools.wraps(set_size)
        def watched_set_size(self, w, h=None, forward=True):
            try:
                before = self.get_size_inches()
                after = (w, h) if h is not None else tuple(w)
                if not state.get("drawing") and (float(after[0]), float(after[1])) != (float(before[0]), float(before[1])):
                    frame = sys._getframe(1)
                    while frame is not None and in_folder(frame, "matplotlib"):
                        frame = frame.f_back
                    if frame is not None and not in_folder(frame, "seaborn"):
                        self._gridcraft_fixed = True
            except Exception:
                self._gridcraft_fixed = True
            return set_size(self, w, h, forward=forward)

        figure.__init__ = watched_init
        figure.set_size_inches = watched_set_size

    sys.meta_path.insert(0, PlotHook)

    def table(rows, headers):
        # A range is a pandas DataFrame, as in Excel; `headers=True` makes its first row the column
        # names. Without pandas it stays a list of rows.
        try:
            import pandas
        except Exception:
            return rows
        if headers and rows:
            return date_columns(pandas, pandas.DataFrame(rows[1:], columns=rows[0]))
        return date_columns(pandas, pandas.DataFrame(rows))

    def date_columns(pandas, frame):
        # pandas keeps datetime.date values as objects, and some versions datetime.datetime values
        # past its range too. A column of dates and empty cells becomes a datetime column, in the
        # units it can hold them in. Past what this pandas can do, or with something else in the
        # column, it stays as it is: the values are right either way.
        try:
            for i in range(frame.shape[1]):
                column = frame.iloc[:, i]
                if column.dtype == object and holds_only_dates(column.tolist()):
                    frame.isetitem(i, as_dates(pandas, column))
        except Exception:
            pass
        return frame

    def holds_only_dates(values):
        found = False
        for x in values:
            if isinstance(x, datetime.date):
                found = True
            elif x is not None:
                return False
        return found

    def as_dates(pandas, column):
        try:
            return pandas.to_datetime(column)
        except Exception:
            import numpy

            return pandas.Series(numpy.array(column.tolist(), dtype="datetime64[us]"), index=column.index)

    def xl(reference, headers=False):
        """xl(reference, headers=False): the value of a cell, or a range as a pandas DataFrame (a
        list of rows when pandas isn't installed). With headers=True the first row names the
        columns. A cell formatted as a date or a time is a datetime.date, datetime.time or
        datetime.datetime. The reference must be written as text in the cell's code, such as
        xl("A1:B5")."""
        inputs = state["inputs"]
        if inputs is None:
            raise RuntimeError("xl() only works while a cell runs")
        if not isinstance(reference, str):
            raise XlReference('xl() takes a reference written as text, such as xl("A1:B5")')
        entry = inputs.get(reference)
        if not isinstance(entry, dict):
            raise XlReference(
                'xl("%s"): only references written as text in this cell\'s code can be read' % cut_chars(reference, 100)
            )
        value = to_py(entry.get("value"))
        return table(value, headers) if isinstance(value, list) else value

    # Results: Python to wire values.
    NOT_SCALAR = object()

    def item(x):
        # A numpy scalar as the Python value it holds. A numpy date or duration stays what it is: its
        # own item() gives a bare integer in nanoseconds or past Python's years.
        get = getattr(x, "item", None)
        if get is not None and getattr(x, "shape", None) == ():
            if getattr(getattr(x, "dtype", None), "kind", None) in ("M", "m"):
                return x[()]
            try:
                return get()
            except Exception:
                return x
        return x

    def blank(x):
        # pandas's missing values are empty cells.
        x = item(x)
        if type(x).__name__ in ("NaTType", "NAType"):
            return None
        try:
            return None if x != x else x
        except Exception:
            return x

    def default_index(index):
        return type(index).__name__ == "RangeIndex" and index.start == 0 and index.step == 1

    def rows_of(v):
        """A pandas or numpy result as rows, or None for anything else. A table's column names are
        its first row, and its row labels its first column unless they are just 0, 1, 2..."""
        module = (type(v).__module__ or "").split(".")[0]
        name = type(v).__name__
        if module == "pandas" and name == "DataFrame":
            if v.shape[0] * v.shape[1] > MAX_ARRAY_CELLS:
                raise TooBig("A result can have at most %d cells" % MAX_ARRAY_CELLS)
            labels = not default_index(v.index)
            rows = [([v.index.name if v.index.name is not None else ""] if labels else []) + [item(c) for c in v.columns]]
            for label, row in zip(v.index, v.itertuples(index=False, name=None)):
                rows.append(([item(label)] if labels else []) + [blank(x) for x in row])
            return rows
        if module == "pandas" and name == "Series":
            if len(v) > MAX_ARRAY_CELLS:
                raise TooBig("A result can have at most %d cells" % MAX_ARRAY_CELLS)
            labels = not default_index(v.index)
            return [([item(label)] if labels else []) + [blank(x)] for label, x in zip(v.index, v.tolist())]
        if module == "pandas" and name in ("DatetimeIndex", "TimedeltaIndex"):
            if len(v) > MAX_ARRAY_CELLS:
                raise TooBig("A result can have at most %d cells" % MAX_ARRAY_CELLS)
            return [[x] for x in v.tolist()]
        if module == "numpy" and name == "ndarray" and v.ndim in (1, 2):
            if v.size > MAX_ARRAY_CELLS:
                raise TooBig("A result can have at most %d cells" % MAX_ARRAY_CELLS)
            if v.dtype.kind in ("M", "m"):
                # tolist() gives bare integers for nanoseconds: the elements keep their units.
                return [[x] for x in v] if v.ndim == 1 else [list(r) for r in v]
            return [[x] for x in v.tolist()] if v.ndim == 1 else v.tolist()
        return None

    def number(x):
        try:
            f = float(x)
        except (OverflowError, ValueError, TypeError):
            return {"error": "#NUM!"}
        return f if math.isfinite(f) else {"error": "#NUM!"}

    # Dates and durations, for a result. What Python can spell goes out as it is; GridCraft alone turns
    # it into a serial number, and gives #NUM! for what a serial can't hold.
    DAY_ATTOSECONDS = 86400 * 10**18
    NUMPY_UNITS = {
        "W": 7 * DAY_ATTOSECONDS,
        "D": DAY_ATTOSECONDS,
        "h": 3600 * 10**18,
        "m": 60 * 10**18,
        "s": 10**18,
        "ms": 10**15,
        "us": 10**12,
        "ns": 10**9,
        "ps": 10**6,
        "fs": 10**3,
        "as": 1,
    }
    NUMPY_NAT = -(2**63)

    def day_text(v):
        return "%04d-%02d-%02d" % (v.year, v.month, v.day)

    def clock_text(v):
        text = "%02d:%02d:%02d" % (v.hour, v.minute, v.second)
        return text + (".%06d" % v.microsecond if v.microsecond else "")

    def date_on_wire(key, text):
        return {key: text} if len(text) <= MAX_DATE_BYTES else {"error": "#NUM!"}

    def moment_on_wire(v, date_only_at_midnight):
        # The attributes are the local wall-clock time, whatever time zone the value has: Excel has
        # no time zones.
        clock = clock_text(v)
        if date_only_at_midnight and clock == "00:00:00":
            return date_on_wire("date", day_text(v))
        return date_on_wire("dateTime", day_text(v) + "T" + clock)

    def numpy_ticks(x):
        # How many of its unit a numpy date or duration holds, as a Python integer, since numpy's own
        # conversions between units overflow without a word. None for NaT.
        unit, count = sys.modules["numpy"].datetime_data(x.dtype)
        n = int(x.astype("int64"))
        return None if n == NUMPY_NAT else (unit, n * count)

    def numpy_date_on_wire(x):
        ticks = numpy_ticks(x)
        if ticks is None:
            return None
        unit, n = ticks
        try:
            if unit in ("Y", "M"):
                years, months = (n, 0) if unit == "Y" else divmod(n, 12)
                return date_on_wire("date", day_text(datetime.date(1970 + years, months + 1, 1)))
            days, rest = divmod(n * NUMPY_UNITS[unit], DAY_ATTOSECONDS)
            moment = datetime.datetime(1970, 1, 1) + datetime.timedelta(days=days, microseconds=rest // 10**12)
        except (KeyError, OverflowError, ValueError):
            # Past Python's years: no serial holds it either.
            return {"error": "#NUM!"}
        return moment_on_wire(moment, True)

    def numpy_duration_on_wire(x):
        ticks = numpy_ticks(x)
        if ticks is None:
            return None
        unit, n = ticks
        # Years and months have no length in days.
        if unit not in NUMPY_UNITS:
            return {"error": "#NUM!"}
        try:
            return {"duration": n * NUMPY_UNITS[unit] / DAY_ATTOSECONDS}
        except OverflowError:
            return {"error": "#NUM!"}

    def date_scalar(v):
        """A date, a time or a duration in its wire form, or NOT_SCALAR for anything else. pandas's
        Timestamp, Timedelta and NaT are subclasses of the datetime classes. pandas and numpy have
        no type for a date without a time, so midnight there means just the date."""
        name = type(v).__name__
        if name == "NaTType":
            return None
        if (type(v).__module__ or "").split(".")[0] == "numpy":
            if name == "datetime64":
                return numpy_date_on_wire(v)
            return numpy_duration_on_wire(v) if name == "timedelta64" else NOT_SCALAR
        if isinstance(v, datetime.datetime):
            return moment_on_wire(v, name == "Timestamp")
        if isinstance(v, datetime.date):
            return date_on_wire("date", day_text(v))
        if isinstance(v, datetime.time):
            return date_on_wire("time", clock_text(v))
        if isinstance(v, datetime.timedelta):
            return {"duration": float(v / datetime.timedelta(days=1))}
        return NOT_SCALAR

    def scalar(v):
        v = item(v)
        if v is None:
            return None
        if isinstance(v, bool):
            return v
        if isinstance(v, (int, float)):
            return number(v)
        if isinstance(v, str):
            if len(v) * 4 > MAX_TEXT_BYTES and len(str.encode(v, "utf-8", "replace")) > MAX_TEXT_BYTES:
                raise TooBig("A text result can have at most %d bytes" % MAX_TEXT_BYTES)
            return str(v)
        if isinstance(v, ExcelError):
            return {"error": v.text} if v.text in ERRORS else {"error": "#VALUE!"}
        try:
            return date_scalar(v)
        except Exception:
            return {"error": "#VALUE!"}

    def safe_repr(v):
        try:
            return str(repr(v))
        except BaseException:
            return "<%s object>" % type_name(v)

    def type_name(v):
        try:
            return cut_bytes(str(type(v).__name__), MAX_TYPE_NAME_BYTES)
        except BaseException:
            return "object"

    def element(x):
        s = scalar(x)
        return cut_chars(safe_repr(x), MAX_PREVIEW_CHARS) if s is NOT_SCALAR else s

    def grid(rows):
        cols = max((len(r) for r in rows), default=0)
        if cols == 0:
            return {"error": "#CALC!"}
        if len(rows) * cols > MAX_ARRAY_CELLS:
            raise TooBig("A result can have at most %d cells" % MAX_ARRAY_CELLS)
        data = []
        for r in rows:
            data.extend(element(x) for x in r)
            # Short rows are padded with #N/A, as Excel pads arrays.
            data.extend({"error": "#N/A"} for _ in range(cols - len(r)))
        return {"array": {"rows": len(rows), "cols": cols, "data": data}}

    def instance_of(v, module, name):
        # Only a package that is loaded can have made one, and a subclass counts.
        cls = getattr(sys.modules.get(module), name, None)
        return isinstance(cls, type) and isinstance(v, cls)

    def size_of(v):
        """What a Python object's name says about its size, or None when it has none to say."""
        if instance_of(v, "pandas", "DataFrame") or instance_of(v, "numpy", "ndarray"):
            dims = v.shape
        elif instance_of(v, "pandas", "Series") or isinstance(v, (list, tuple, dict, set, frozenset)):
            dims = (len(v),)
        else:
            return None
        dims = [int(n) for n in dims]
        if not 1 <= len(dims) <= MAX_SIZE_DIMS or not all(0 <= n <= MAX_SIZE for n in dims):
            return None
        return dims

    def wire_bytes(v):
        return len(encode(v).encode("utf-8", "replace")) + 1

    def preview_table(names, rows, total_rows, total_cols, cols, labels):
        """A table's preview, within the caps of protocol.rs. `rows` gives its rows, each already at
        most `cols` long: a short one is padded with #N/A, as a spill is. A long text is cut, and rows
        stop at the budget in bytes, and the totals say what is missing."""
        if total_rows > MAX_SIZE or total_cols > MAX_SIZE:
            return None
        cut = []

        def text(s):
            shown = cut_bytes(s, MAX_TABLE_TEXT_BYTES)
            if len(shown) != len(s):
                cut.append(True)
            return shown

        def name(n):
            n = item(n)
            if n is None:
                return ""
            try:
                return text(str(n))
            except BaseException:
                return text(safe_repr(n))

        def cell(x):
            x = item(x)
            try:
                if isinstance(x, str):
                    return text(str(x))
                s = scalar(x)
            except BaseException:
                s = NOT_SCALAR
            if s is not NOT_SCALAR:
                return s
            whole = safe_repr(x)
            shown = cut_chars(whole, MAX_PREVIEW_CHARS)
            if len(shown) != len(whole):
                cut.append(True)
            return shown

        names = [name(n) for n in names] if names is not None else []
        spent = sum(wire_bytes(n) for n in names)
        data = []
        count = 0
        for row in rows:
            cells = [cell(x) for x in row]
            cells.extend({"error": "#N/A"} for _ in range(cols - len(cells)))
            spent += sum(wire_bytes(c) for c in cells)
            if spent > MAX_TABLE_BYTES:
                break
            data.extend(cells)
            count += 1
        table = {"rows": count, "cols": cols, "data": data, "totalRows": total_rows, "totalCols": total_cols}
        if any(names):
            table["columns"] = names
        if labels:
            table["rowLabels"] = True
        if cut:
            table["textCut"] = True
        return table

    def frame_table(v):
        # As a spill has it: the column names, and the row labels unless they are just 0, 1, 2...
        labels = not default_index(v.index)
        part = v.iloc[: MAX_TABLE_ROWS, : MAX_TABLE_COLS - (1 if labels else 0)]
        names = ([v.index.name] if labels else []) + list(part.columns)

        def rows():
            for label, row in zip(part.index, part.itertuples(index=False, name=None)):
                yield ([label] if labels else []) + [blank(x) for x in row]

        return preview_table(names, rows(), int(v.shape[0]), int(v.shape[1]), len(names), labels)

    def series_table(v):
        labels = not default_index(v.index)
        part = v.iloc[: MAX_TABLE_ROWS]
        names = ([v.index.name] if labels else []) + [v.name]

        def rows():
            for label, x in zip(part.index, part.tolist()):
                yield ([label] if labels else []) + [blank(x)]

        return preview_table(names, rows(), len(v), 1, len(names), labels)

    def array_table(v):
        part = v[: MAX_TABLE_ROWS, : MAX_TABLE_COLS]
        # tolist() gives bare integers for nanoseconds: the elements keep their units.
        rows = [list(r) for r in part] if part.dtype.kind in ("M", "m") else part.tolist()
        return preview_table(None, rows, int(v.shape[0]), int(v.shape[1]), int(part.shape[1]), False)

    def rows_table(v):
        cols = max(len(r) for r in v)
        rows = (list(r[: MAX_TABLE_COLS]) for r in v[: MAX_TABLE_ROWS])
        return preview_table(None, rows, len(v), cols, min(cols, MAX_TABLE_COLS), False)

    def table_of(v):
        """A bounded look at a table-like object, or None for anything else."""
        if instance_of(v, "pandas", "DataFrame"):
            return frame_table(v)
        if instance_of(v, "pandas", "Series"):
            return series_table(v)
        if instance_of(v, "numpy", "ndarray"):
            return array_table(v) if v.ndim == 2 else None
        if isinstance(v, (list, tuple)) and len(v) and all(isinstance(r, (list, tuple)) for r in v):
            return rows_table(v)
        return None

    def unless_it_fails(find, v):
        # The object's own methods run here, and the cell has its result whatever they do.
        try:
            return find(v)
        except BaseException:
            return None

    def as_object(v, sized):
        o = {"typeName": type_name(v), "preview": cut_chars(safe_repr(v), MAX_PREVIEW_CHARS)}
        if sized:
            size = unless_it_fails(size_of, v)
            table = unless_it_fails(table_of, v)
            if size:
                o["size"] = size
            if table:
                o["table"] = table
        return {"object": o}

    # Pictures. A figure is drawn at twice its own resolution, so it stays sharp on a dense screen and
    # when it is enlarged; `size` is what its own resolution gives, as Excel reports it.
    DENSITY = 2

    class PictureTooLarge(ValueError):
        pass

    PictureTooLarge.__name__ = "ValueError"
    PictureTooLarge.__qualname__ = "ValueError"

    def too_large(width, height):
        return not (0 < width <= MAX_PICTURE_SIDE and 0 < height <= MAX_PICTURE_SIDE and width * height <= MAX_PICTURE_PIXELS)

    def too_large_message(width, height):
        return "This picture would be %d x %d pixels. The limit is %d pixels a side and %d pixels in all. Make figsize or dpi smaller." % (
            width,
            height,
            MAX_PICTURE_SIDE,
            MAX_PICTURE_PIXELS,
        )

    def root_figure(fig):
        # A subfigure belongs to a figure, which is what can be drawn.
        for _ in range(16):
            if instance_of(fig, "matplotlib.figure", "Figure"):
                return fig
            fig = getattr(fig, "figure", None)
            if fig is None:
                return None
        return None

    def figure_of(v, depth=0):
        """The matplotlib figure a value stands for, or None. Only a package that is loaded can have
        made one. A plot call's result counts: a line, a list of lines, the tuple plt.hist gives, the
        dictionary plt.boxplot gives, a seaborn grid, and an array of axes."""
        if "matplotlib.figure" not in sys.modules or depth > 3:
            return None
        if instance_of(v, "matplotlib.figure", "FigureBase"):
            return root_figure(v)
        if instance_of(v, "matplotlib.artist", "Artist") or instance_of(v, "seaborn.axisgrid", "_BaseGrid"):
            return root_figure(getattr(v, "figure", None))
        # What else a plot call gives, such as plt.pie's container: the figure it drew on.
        if (type(v).__module__ or "").split(".")[0] == "matplotlib":
            return changed_figure()
        if isinstance(v, tuple):
            for x in v[:8]:
                fig = figure_of(x, depth + 1)
                if fig is not None:
                    return fig
            return None
        if isinstance(v, list):
            return figure_of(v[0], depth + 1) if v else None
        if isinstance(v, dict):
            for x in v.values():
                return figure_of(x, depth + 1)
            return None
        if instance_of(v, "numpy", "ndarray") and v.dtype == object and v.size:
            return figure_of(v.flat[0], depth + 1)
        return None

    def current_figures():
        helpers = sys.modules.get("matplotlib._pylab_helpers")
        return [m.canvas.figure for m in helpers.Gcf.get_all_fig_managers()] if helpers is not None else []

    def changed_figure():
        # pyplot's current figure, when this cell changed it.
        helpers = sys.modules.get("matplotlib._pylab_helpers")
        manager = helpers.Gcf.get_active() if helpers is not None else None
        fig = manager.canvas.figure if manager is not None else None
        return fig if fig is not None and fig.stale else None

    def draw_figure(fig, dpi, tight):
        buffer = io.BytesIO()
        options = {"bbox_inches": "tight"} if tight else {}
        state["drawing"] = True
        try:
            fig.savefig(buffer, format="png", dpi=dpi, metadata={"Software": None}, **options)
        finally:
            state["drawing"] = False
        return buffer.getvalue()

    def is_free(fig):
        """Whether the figure's size is up to GridCraft: the code didn't choose one in any way."""
        matplotlib = sys.modules["matplotlib"]
        default = list(matplotlib.rcParamsDefault["figure.figsize"])
        return not getattr(fig, "_gridcraft_fixed", False) and list(matplotlib.rcParams["figure.figsize"]) == default

    def own_layout(fig):
        # A layout engine of the figure's own, which lays it out at any size. tight_layout() leaves a
        # placeholder that does nothing.
        engine = fig.get_layout_engine()
        return engine is not None and type(engine).__name__ != "PlaceHolderLayoutEngine"

    def spans(rects, axis, size):
        """Along one axis, in inches: where the plot areas are, and how much lies outside them. What
        lies between plot areas holds labels, which keep their room whatever the size."""
        ranges = [(getattr(r, axis + "0") * size, getattr(r, axis + "1") * size) for r in rects]
        edges = sorted({0.0, float(size)} | {e for pair in ranges for e in pair})
        pieces = [(a, b, any(lo <= (a + b) / 2 <= hi for lo, hi in ranges)) for a, b in zip(edges, edges[1:])]
        return edges, pieces, sum(b - a for a, b, covered in pieces if not covered)

    def stretched(rects, axis, old, new):
        """Maps positions along one axis to the new size: plot areas stretch, and the gaps don't. None
        when the gaps alone don't fit."""
        edges, pieces, fixed = spans(rects, axis, old)
        areas = old - fixed
        if areas <= 0 or new <= fixed:
            return None
        scale = (new - fixed) / areas
        placed = [0.0]
        for a, b, covered in pieces:
            placed.append(placed[-1] + (b - a) * (scale if covered else 1.0))
        return lambda x: float(sys.modules["numpy"].interp(x, edges, placed))

    def free_axes(fig):
        return [a for a in fig.axes if a.get_axes_locator() is None]

    def figure_floor(fig):
        """The smallest size, in the pixels of `size`, at which the figure is drawn again: half its
        natural size, and enough for what lies outside its plot areas. Below it, a text would run past
        the edge, so the picture is scaled instead. None for a figure that isn't free."""
        if not is_free(fig):
            return None
        dpi = float(fig.get_dpi())
        width, height = (float(x) for x in fig.get_size_inches())
        rects = [a.get_position(original=True) for a in free_axes(fig)] if not own_layout(fig) else []
        gap_x = spans(rects, "x", width)[2] if rects else 0.0
        gap_y = spans(rects, "y", height)[2] if rects else 0.0
        return [max(1, math.ceil(max(width / 2, gap_x + 0.4) * dpi)), max(1, math.ceil(max(height / 2, gap_y + 0.4) * dpi))]

    def reflow(fig, width, height):
        """The figure drawn at exactly `width` x `height`, in the pixels of `size`, and how many pixels
        stand for one of those. Text keeps its size and the plot areas take the new shape. The figure
        is left as it was, for the cells after this one."""
        dpi = float(fig.get_dpi())
        old_w, old_h = (float(x) for x in fig.get_size_inches())
        new_w, new_h = width / dpi, height / dpi
        axes = free_axes(fig)
        saved = [(a, a.get_position(original=True).frozen(), a.get_position().frozen(), a.get_in_layout()) for a in fig.axes]
        state["drawing"] = True
        try:
            if not own_layout(fig):
                rects = [a.get_position(original=True) for a in axes]
                map_x, map_y = stretched(rects, "x", old_w, new_w), stretched(rects, "y", old_h, new_h)
                if rects and (map_x is None or map_y is None):
                    raise PictureTooLarge("This picture can't be drawn so small.")
                for a, r in zip(axes, rects):
                    x0, x1 = map_x(r.x0 * old_w), map_x(r.x1 * old_w)
                    y0, y1 = map_y(r.y0 * old_h), map_y(r.y1 * old_h)
                    a.set_position([x0 / new_w, y0 / new_h, (x1 - x0) / new_w, (y1 - y0) / new_h])
            fig.set_size_inches(new_w, new_h, forward=False)
            density = 1 if too_large(width * DENSITY, height * DENSITY) else DENSITY
            data = draw_figure(fig, dpi * density, False)
            if len(data) > MAX_PICTURE_BYTES and density > 1:
                density = 1
                data = draw_figure(fig, dpi, False)
            return data, density
        finally:
            fig.set_size_inches(old_w, old_h, forward=False)
            for a, original, active, in_layout in saved:
                a.set_position(original, which="original")
                a.set_position(active, which="active")
                a.set_in_layout(in_layout)
            state["drawing"] = False
            fig.stale = False

    def picture_of_figure(fig):
        """The figure's bytes, drawn as a picture, and how many pixels stand for one of its own."""
        dpi = float(fig.get_dpi())
        inches = fig.get_size_inches()
        width, height = float(inches[0]) * dpi, float(inches[1]) * dpi
        if too_large(width, height):
            raise PictureTooLarge(too_large_message(width, height))
        # What sticks out of the figure makes a tight picture larger: anything far out would ask for
        # more memory than the worker has.
        get_renderer = getattr(fig.canvas, "get_renderer", None)
        if get_renderer is not None:
            box = fig.get_tightbbox(get_renderer())
            width, height = (float(box.width) + 0.2) * dpi, (float(box.height) + 0.2) * dpi
            if too_large(width, height):
                raise PictureTooLarge(too_large_message(width, height))
        density = 1 if too_large(width * DENSITY, height * DENSITY) else DENSITY
        data = draw_figure(fig, dpi * density, True)
        if len(data) > MAX_PICTURE_BYTES and density > 1:
            density = 1
            data = draw_figure(fig, dpi, True)
        fig.stale = False
        return data, density

    def displayed(v):
        """What an object's own _repr_png_ or _repr_jpeg_ gives, or None."""
        for name in ("_repr_png_", "_repr_jpeg_"):
            method = getattr(type(v), name, None)
            if method is None:
                continue
            data = method(v)
            if isinstance(data, tuple) and data:
                data = data[0]
            if isinstance(data, (bytes, bytearray)) and data:
                return bytes(data)
        return None

    JPEG_FRAMES = frozenset(range(0xC0, 0xD0)) - {0xC4, 0xC8, 0xCC}

    def picture_format(data):
        """The type, width and height a picture's header gives, or None for anything else."""
        if data[:8] == b"\x89PNG\r\n\x1a\n" and data[12:16] == b"IHDR" and len(data) >= 24:
            return "image/png", int.from_bytes(data[16:20], "big"), int.from_bytes(data[20:24], "big")
        if data[:2] != b"\xff\xd8":
            return None
        i = 2
        while i + 9 <= len(data):
            if data[i] != 0xFF:
                return None
            marker = data[i + 1]
            if marker == 0xFF:
                i += 1
            elif marker in JPEG_FRAMES:
                return "image/jpeg", int.from_bytes(data[i + 7:i + 9], "big"), int.from_bytes(data[i + 5:i + 7], "big")
            elif marker == 0x01 or 0xD0 <= marker <= 0xD8:
                i += 2
            else:
                i += 2 + max(2, int.from_bytes(data[i + 2:i + 4], "big"))
        return None

    def picture_on_wire(data, density, floor=None):
        found = picture_format(data)
        if found is None:
            return None
        mime, width, height = found
        if too_large(width, height):
            raise PictureTooLarge(too_large_message(width, height))
        if len(data) > MAX_PICTURE_BYTES:
            raise PictureTooLarge("This picture takes more than %d bytes. Make figsize or dpi smaller." % MAX_PICTURE_BYTES)
        image = {"mime": mime, "base64": base64.b64encode(data).decode("ascii")}
        if density != 1:
            image["density"] = density
        if floor is not None:
            image["floor"] = floor
        size = [max(1, width // density), max(1, height // density)]
        return {"object": {"typeName": "Image", "preview": "Image", "size": size, "image": image}}

    def picture_result(v, at):
        """A cell's result as a picture, or None for any other result. A value that can give a
        picture does; with no value, a figure this cell changed does; a number or a table takes
        priority over an open figure."""
        fig = unless_it_fails(figure_of, v)
        if fig is None and v is not None:
            data = unless_it_fails(displayed, v)
            return picture_on_wire(data, 1) if data is not None else None
        if fig is None:
            fig = changed_figure()
        if fig is None:
            return None
        if at is not None:
            kept[at] = fig
        data, density = picture_of_figure(fig)
        return picture_on_wire(data, density, unless_it_fails(figure_floor, fig))

    def render(req):
        # A cell's figure at a new size, without running the cell.
        rid = req.get("id")
        at = position(req)
        width, height = req.get("width"), req.get("height")
        fig = kept.get(at)
        try:
            if fig is None or not isinstance(width, int) or not isinstance(height, int):
                raise ValueError("No figure to draw")
            if too_large(width, height):
                raise PictureTooLarge(too_large_message(width, height))
            data, density = reflow(fig, width, height)
            return {"type": "done", "id": rid, "ok": True, "value": picture_on_wire(data, density, figure_floor(fig))}
        except BaseException as e:  # noqa: BLE001 - the picture stays as it is
            return done_error(rid, error_message(e), None)

    # Which cell made each figure, and the figure each cell gave. A cell runs again from a clean
    # start: the figures it or a later cell made are closed, or lines would pile up on every run.
    # Figures of earlier cells stay, so a later cell can add a title.
    FIRST = (-1, -1, -1)
    kept = {}

    def position(req):
        at = req.get("at")
        if isinstance(at, list) and len(at) == 3 and all(isinstance(n, int) for n in at):
            return tuple(at)
        return None

    def before_cell(at):
        for fig in current_figures():
            owner = getattr(fig, "_gridcraft_at", FIRST)
            if at is not None and owner >= at:
                sys.modules["matplotlib.pyplot"].close(fig)
            else:
                fig.stale = False
        kept.pop(at, None)

    def after_cell(at):
        """Marks the figures this cell made, and returns the first cell before it whose figure this
        one changed: running this cell again has to start from there."""
        touched = None
        for fig in current_figures():
            owner = getattr(fig, "_gridcraft_at", None)
            if owner is None:
                fig._gridcraft_at = at if at is not None else FIRST
            elif at is not None and FIRST < owner < at and fig.stale and (touched is None or owner < touched):
                touched = owner
        return touched

    def reset_plots():
        kept.clear()
        pyplot = sys.modules.get("matplotlib.pyplot")
        if pyplot is not None:
            pyplot.close("all")
            sys.modules["matplotlib"].rcdefaults()
            use_non_latin_fonts()

    def output(v, want_object):
        s = scalar(v)
        if s is not NOT_SCALAR:
            return s
        if want_object:
            return as_object(v, True)
        rows = rows_of(v)
        if rows is not None:
            return grid(rows) if rows else {"error": "#CALC!"}
        if isinstance(v, (list, tuple)):
            if not v:
                return {"error": "#CALC!"}
            if all(isinstance(r, (list, tuple)) for r in v):
                return grid(v)
            # A flat list is one column.
            if len(v) > MAX_ARRAY_CELLS:
                raise TooBig("A result can have at most %d cells" % MAX_ARRAY_CELLS)
            return grid([[x] for x in v])
        return as_object(v, False)

    # Printed output, kept up to the cap.
    class Capture(io.TextIOBase):
        CUT = "\n[output cut off]\n"

        def __init__(self):
            self.parts = []
            self.left = MAX_STDOUT_BYTES - len(self.CUT)
            self.cut = False

        def writable(self):
            return True

        def write(self, s):
            s = str(s)
            if self.cut:
                return len(s)
            b = s.encode("utf-8", "replace")
            if len(b) <= self.left:
                self.parts.append(s)
                self.left -= len(b)
            else:
                self.parts.append(b[: self.left].decode("utf-8", "ignore"))
                self.parts.append(self.CUT)
                self.left = 0
                self.cut = True
            return len(s)

        def text(self):
            return "".join(self.parts)

    def error_line(e):
        if isinstance(e, SyntaxError) and e.filename == "<cell>":
            return e.lineno
        line = None
        tb = e.__traceback__
        while tb is not None:
            if tb.tb_frame.f_code.co_filename == "<cell>":
                line = tb.tb_lineno
            tb = tb.tb_next
        return line

    def error_message(e):
        name = type_name(e)
        if isinstance(e, SyntaxError):
            return "%s: %s" % (name, e.msg)
        try:
            text = str(e)
        except BaseException:
            text = ""
        return "%s: %s" % (name, text) if text else name

    # What kind of failure an error is, in the words of the diagnostics catalog (diagnostics.rs), so
    # the person is told what to do about it. A guess is fine: the message goes along with it.
    NETWORK_FILES = ("socket.py", "ssl.py", "selectors.py", "connection.py", "connectionpool.py", "client.py", "request.py", "adapters.py")
    NETWORK_FOLDERS = ("asyncio", "urllib", "urllib3", "http", "requests", "httpx", "aiohttp")
    PROCESS_FILES = ("subprocess.py", "popen_fork.py", "popen_spawn_posix.py", "popen_forkserver.py", "spawn.py")
    PROCESS_FOLDERS = ("multiprocessing", "joblib", "loky")
    NETWORK_WORDS = ("socket", "connect", "urlopen", "getaddrinfo")
    PROCESS_WORDS = ("fork", "exec", "spawn", "system(", "popen", "Popen", "subprocess")

    def chain(e):
        seen = 0
        while e is not None and seen < 16:
            yield e
            seen += 1
            link = e.__cause__ or e.__context__ or getattr(e, "reason", None)
            e = link if isinstance(link, BaseException) else None

    def denied_kind(e, code):
        """Which kind of denial a permission error is, from where it was raised."""
        tb = e.__traceback__
        line = None
        while tb is not None:
            name = tb.tb_frame.f_code.co_filename
            base = os.path.basename(name)
            parts = name.replace("\\", "/").split("/")
            if base in PROCESS_FILES or any(f in parts for f in PROCESS_FOLDERS):
                return "denied.process"
            if base in NETWORK_FILES or any(f in parts for f in NETWORK_FOLDERS):
                return "denied.network"
            if name == "<cell>":
                line = tb.tb_lineno
            tb = tb.tb_next
        lines = code.split("\n") if isinstance(code, str) else []
        text = lines[line - 1] if isinstance(line, int) and 0 < line <= len(lines) else ""
        if any(w in text for w in PROCESS_WORDS):
            return "denied.process"
        if any(w in text for w in NETWORK_WORDS):
            return "denied.network"
        return "denied.file"

    def classify(e, code):
        """The catalog's kind for an error, and what it was about."""
        if isinstance(e, SyntaxError):
            return "python.syntax", None
        if isinstance(e, XlReference):
            return "xl.notLiteral", None
        if isinstance(e, PictureTooLarge):
            return "plot.tooLarge", None
        if isinstance(e, ModuleNotFoundError):
            return "module.missing", e.name
        # seaborn asks for scipy in its own words.
        if isinstance(e, RuntimeError) and "requires scipy" in str(e):
            return "module.missing", "scipy"
        if isinstance(e, ImportError):
            text = str(e)
            if "sandbox" in text or "not permitted" in text or "Permission denied" in text:
                return "module.blockedLibrary", getattr(e, "name", None)
            return "python.exception", None
        if isinstance(e, NameError):
            return "python.name", getattr(e, "name", None)
        for link in chain(e):
            if isinstance(link, PermissionError) or (isinstance(link, OSError) and link.errno in (errno.EPERM, errno.EACCES)):
                kind = denied_kind(link, code)
                return kind, str(link.filename) if kind == "denied.file" and link.filename else None
            if isinstance(link, socket_errors()):
                return "denied.network", None
        if isinstance(e, FileNotFoundError) and "No usable temporary directory" in str(e):
            return "denied.file", None
        return "python.exception", None

    def socket_errors():
        s = sys.modules.get("socket")
        return (s.gaierror,) if s is not None else ()

    def compile_cell(code):
        tree = ast.parse(code, "<cell>", "exec")
        last = None
        if tree.body and isinstance(tree.body[-1], ast.Expr):
            last = ast.Expression(tree.body.pop().value)
        body = compile(tree, "<cell>", "exec")
        return body, (compile(last, "<cell>", "eval") if last is not None else None)

    # Initialization code. What a line binds, and which package a line that can't run is about: that
    # is reported, and an empty answer is a line left out without a word. None means the error is the
    # code's own.
    def bound_by(stmt):
        if isinstance(stmt, (ast.Import, ast.ImportFrom)):
            return [a.asname or a.name.split(".")[0] for a in stmt.names]
        return [n.id for n in ast.walk(stmt) if isinstance(n, ast.Name) and isinstance(n.ctx, ast.Store)]

    def modules_of(stmt):
        if isinstance(stmt, ast.Import):
            return [a.name.split(".")[0] for a in stmt.names]
        if isinstance(stmt, ast.ImportFrom) and stmt.level == 0:
            return [(stmt.module or "").split(".")[0]]
        return []

    def module_call(stmt, g):
        # A call on a module at the top level, such as warnings.simplefilter("ignore"): what setting
        # an option looks like. The module's name, or None.
        call = stmt.value if isinstance(stmt, ast.Expr) else None
        if not isinstance(call, ast.Call) or not isinstance(call.func, ast.Attribute):
            return None
        root = call.func
        while isinstance(root, ast.Attribute):
            root = root.value
        if isinstance(root, ast.Name) and isinstance(g.get(root.id), (types.ModuleType, Lazy)):
            return root.id
        return None

    def left_to_lazy(stmt):
        # What the worker already gives lazily: Excel's default initialization imports these, and
        # doing so would only cost the time it takes.
        if not isinstance(stmt, ast.Import):
            return False
        for alias in stmt.names:
            top = alias.name.split(".")[0]
            if DEFAULT_IMPORTS.get(alias.asname or top, "").split(".")[0] != top:
                return False
        return True

    def why_skipped(e, stmt, gone, g, from_file):
        if isinstance(e, NameError) and getattr(e, "name", None) in gone:
            return gone[e.name]
        modules = modules_of(stmt)
        if not from_file:
            # A person's code can keep Excel's own options: only that module's are excused.
            return "excel" if isinstance(e, AttributeError) and module_call(stmt, g) == "excel" else None
        if modules:
            name = getattr(e, "name", None)
            return (name if isinstance(name, str) and name else modules[0]).split(".")[0]
        if isinstance(e, ImportError) and e.name:
            return e.name.split(".")[0]
        return module_call(stmt, g) if isinstance(e, Exception) else None

    def initialize(code, from_file):
        tree = ast.parse(code, "<cell>", "exec")
        g = namespace["globals"]
        if not from_file and not any("excel" in modules_of(s) for s in tree.body):
            exec(compile(tree, "<cell>", "exec"), g)
            return None
        skipped = {}
        gone = {}
        for stmt in tree.body:
            if from_file and left_to_lazy(stmt):
                gone.update((name, "") for name in bound_by(stmt) if name not in g)
                continue
            try:
                exec(compile(ast.Module([stmt], []), "<cell>", "exec"), g)
            except BaseException as e:  # noqa: BLE001 - what the rules don't skip is raised again
                why = why_skipped(e, stmt, gone, g, from_file)
                if why is None:
                    raise
                if why:
                    skipped.setdefault(why, stmt.lineno)
                for name in bound_by(stmt):
                    gone.setdefault(name, why)
        return [[module, line] for module, line in skipped.items()] or None

    namespace = {}

    def fresh():
        # The cells share one `__main__`, as in Excel; Reset starts a new one.
        main = types.ModuleType("__main__")
        main.__dict__["__builtins__"] = builtins
        main.__dict__["xl"] = xl
        main.__dict__["excel"] = excel
        for name, module in DEFAULT_IMPORTS.items():
            main.__dict__[name] = Lazy(module)
        sys.modules["__main__"] = main
        namespace["globals"] = main.__dict__

    def run(req):
        rid = req.get("id")
        code = req.get("code")
        inputs = req.get("inputs")
        init = req.get("init")
        capture = Capture()
        saved = sys.stdout
        try:
            if not isinstance(code, str):
                raise TypeError("no code")
            # xl() has no cell to read for while initialization code runs.
            state["inputs"] = None if init else inputs if isinstance(inputs, dict) else {}
            at = None if init else position(req)
            sys.stdout = capture
            try:
                before_cell(at)
                if init:
                    value = initialize(code, init == "file")
                else:
                    body, last = compile_cell(code)
                    g = namespace["globals"]
                    exec(body, g)
                    value = eval(last, g) if last is not None else None
            finally:
                sys.stdout = saved
                state["inputs"] = None
            touched = after_cell(at)
            picture = None if init else picture_result(value, at)
            msg = {"type": "done", "id": rid, "ok": True, "value": picture if picture is not None else output(value, req.get("output") == "object")}
            if touched is not None:
                msg["touched"] = list(touched)
            if capture.text():
                msg["stdout"] = capture.text()
            return msg
        except TooBig as e:
            return done_error(rid, str(e), None, capture.text())
        except BaseException as e:  # noqa: BLE001 - whatever the cell raised is its result
            try:
                kind, subject = classify(e, code)
            except Exception:
                kind, subject = None, None
            return done_error(rid, error_message(e), error_line(e), capture.text(), kind, subject)

    try:
        import resource

        # ru_maxrss counts bytes on macOS and KiB elsewhere.
        peak_unit = 1 if sys.platform == "darwin" else 1024

        def peak():
            return int(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss) * peak_unit
    except Exception:
        peak = None

    def with_peak(msg):
        # The most this process has used, so that a spike counts against the request that made it,
        # however short it was. Left out when it can't be read.
        try:
            if peak is not None:
                msg["peak"] = peak()
        except Exception:
            pass
        return msg

    fresh()
    send({"type": "ready", "selfTest": {"status": "notChecked"}})
    while True:
        line = proto_in.readline()
        if not line:
            break
        try:
            req = decode(line.decode("utf-8"))
        except ValueError:
            break
        kind = req.get("type") if isinstance(req, dict) else None
        if kind == "run":
            send(with_peak(run(req)))
        elif kind == "render":
            send(with_peak(render(req)))
        elif kind == "reset":
            fresh()
            try:
                reset_plots()
            except Exception:
                pass
            send(with_peak({"type": "done", "id": req.get("id"), "ok": True, "value": None}))
        else:
            break
    # GridCraft closed the pipe: stop now, without waiting for threads the cells started.
    os._exit(0)


_gridcraft_worker()
