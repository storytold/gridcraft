# Python in cells

> **Last updated:** 2026-11-06 · **Change:** minor (a cell that says `Image` has a double square that opens the card beside it; earlier: reading a saved file with openpyxl; earlier: the formula bar's PY badge and Run Cell; earlier: saving while cells run; earlier: dates and times, in and out of Python; earlier: what tells you what is wrong: PY tags, the card, the code checks; earlier: the Python tab is contextual; earlier: first version: how to turn Python on, how to set it up with uv, what cells can and can't do, and the limits)

A Python cell runs code you write, in your own Python, on your computer. GridCraft never downloads
Python and never installs packages. It finds the Python you have, and it runs your code in a sandbox
that keeps the code away from everything but the cells it names.

Python runs on macOS and on Linux (x86_64 and aarch64, kernel 5.13 or newer). On Windows, FreeBSD and
the web version GridCraft has no sandbox, so cells don't run there: they keep the values saved in
the file, or show `#BLOCKED!`. The code is still stored and saved.

## Turning Python on

Python is off until you turn it on.

1. Open **File › Options › Python** and tick **Enable Python in cells**.
2. GridCraft looks for a Python. The page shows what it found, with its version and kind.
3. A workbook opened from a file needs its own **Enable**, on the bar that shows under the ribbon.
   Until then its cells keep the values saved in the file. **This Workbook** on the Python tab is the
   same switch. A workbook made in GridCraft needs none.
4. Select a cell, choose **Formulas › Insert Python** (Ctrl+Alt+Shift+P), type `1 + 1`, and press
   Ctrl+Enter. The cell shows `2`.

Only a person can do this, in the app. No file, macro, command, MCP call or control-channel request
can turn Python on, choose the interpreter or enable a workbook. The CLI and MCP never run Python
cells: they read the values saved in the file. Anyone who can change your preferences file, or who
operates your mouse and keyboard, acts as you do; this doesn't protect against that.

## Setting up Python with uv

When GridCraft finds no usable Python, the Options page gives these commands with Copy buttons.
GridCraft copies them; it never runs them. Run them in a terminal, in order.

```sh
curl -LsSf https://astral.sh/uv/install.sh | sh   # only if you don't have uv
uv venv ~/.gridcraft/python --python 3.12
uv pip install --compile-bytecode --python ~/.gridcraft/python numpy pandas matplotlib seaborn statsmodels
```

GridCraft looks in `~/.gridcraft/python` first, so there is nothing more to choose. Then choose
**Check Again**. To add a package later, use **Packages** on the Python tab: type its name and choose
**Copy Install Command**, which gives the uv command for the Python in use.

**Which Python.** Automatic tries, in order: `~/.gridcraft/python`, the active virtual environment,
uv's own Pythons, the active conda environment, `PATH`, and the places Pythons are usually installed.
Python 3.11 or newer is needed. **Interpreter** on the Python tab and the Options page list what was found and
take any other path with **Browse…**. While Python is off GridCraft starts nothing, so the list shows
paths only, without versions.

**Other package managers.** Any Python 3.11 or newer can be chosen by path. The Options page and the
Packages pane have a collapsed section with the pip command.

Packages must be installed inside the Python that GridCraft uses. Packages installed for your user
(`pip install --user`) and editable installs (`pip install -e`) aren't seen.

## What cells can and can't do

**What Python Can Do** on the Python tab lists this for your system, with what enforces each line.

Cells can:

- read cells through `xl("A1:B5")`, with the reference written as text in the code;
- import the packages installed in that Python;
- compute, print (the output is kept in the Run Log) and use threads;
- draw plots with `matplotlib` and `seaborn`, and give any other picture. See [Plots and pictures](#plots-and-pictures).

Cells can't:

- read your files or write files;
- use the network;
- start other programs;
- see your environment variables;
- open windows, or ask for input. `plt.show()` does nothing, and the plot appears in the cell anyway.

Cells run in row order, sheet by sheet in tab order, as in Excel, and share one set of variables.
**Reset** clears them and runs every cell again. Each time Python starts, GridCraft first tries to
read a file outside Python, write a file, open a network connection and start a program, and it
doesn't run Python unless every one is refused. [sandbox.md](sandbox.md) has the details.

## Dates and times

A number in a cell is a date because of its format, and Python sees it that way. `xl("A1")` gives a
`datetime.date`, `datetime.time` or `datetime.datetime`, by itself or in a range. With pandas, a
column of dates and empty cells is a datetime column, with `NaT` for the empty cells. Read a date, add
a day and return it, and the cell shows the next day:

```python
import datetime
xl("A1") + datetime.timedelta(days=1)
```

**What `xl()` gives,** in the workbook's date system (1900 or 1904), to the millisecond:

- a date format gives a `date`, or a `datetime` when the cell holds a time of day too, so nothing is
  lost;
- a date and time format gives a `datetime`;
- a time format gives a `time`, or a `datetime` when the number is 1 or more;
- a format for elapsed time, such as `[h]:mm`, gives the number of days as a number.

A number with no place on the calendar stays a number: one that is negative, one past 31 December
9999, and in the 1900 system the days that never were. Those are serial 60, 29 February 1900, which
that system has for compatibility and Python's calendar doesn't, and anything below 1 under a format
that shows a date. So 28 February 1900 plus a day, in Python, is 1 March 1900, serial 61.

**What comes back.** `date`, `datetime`, `time`, numpy's `datetime64` and pandas's `Timestamp` become
serial numbers in the workbook's date system, in one cell or across a spill, a DataFrame's date column
included. `timedelta`, numpy's `timedelta64` and pandas's `Timedelta` become a number of days. `NaT`
is an empty cell. A value with a time zone uses its local wall-clock time, as Excel has no time zones.

A cell with no format of its own takes one that matches:

- `m/d/yyyy` for a date, and for a `Timestamp` or `datetime64` at midnight, since those types have no
  date without a time;
- `m/d/yyyy h:mm` for a `datetime`, and for every date in a column where some have times;
- `h:mm:ss` for a time;
- `[h]:mm:ss` for a length of time. A negative one stays a plain number of days, as no format shows
  one.

The format goes with the date: when a later run gives no date there, or a spill is blocked, the cell
goes back to General. A format you chose stays, and so does General when you set it on a date a run
gave. Writing a format makes no undo step, like writing the result. In a saved file a format is simply
the cell's format, so after opening it again a run no longer takes it back.

A date a serial can't hold, before 1900 (1904 in a 1904 workbook) or after 9999, gives `#NUM!`, as
does a length of time longer than the whole calendar.

## Python objects and tables

A cell's return type is **Output As** on the Python tab, Ctrl+Alt+Shift+M or the cell menu. With
**Python Object**, a result that isn't a single value stays in one cell, which names it with its size:
`DataFrame 6x4` (rows by columns), `Series 12`, `list 12`, `dict 3`, `ndarray 2x3`. Other objects show
their type's name. What reads the cell reads those words.

**The table on the card.** A DataFrame, a Series, and a 2-D list or array show as a table on the card
that the PY tag or Ctrl+Shift+F5 opens, with their column names and row labels, as they would spill. A
flat list, a dict and other objects show their `repr` instead. The table is what the worker sent, not
the object: at most 200 rows, 30 columns (row labels among them) and 1 MiB, with each text cut at 4 KiB.
When anything is left out, the card says what: how many rows and columns of how many, and whether long
text was cut.

**What you can do with it:**

- **Copy as CSV** and **Copy as Markdown** put the table on the clipboard. In CSV, a text that starts
  with `=`, `+`, `-`, `@`, a tab or a carriage return is quoted with a `'` before it, so a spreadsheet
  doesn't run it as a formula; a number is left as it is. In Markdown, `|` and `\` are escaped and a
  line break is `<br>`.
- **Save as .csv…** asks where, then writes the same CSV as UTF-8.
- **Spill Here** switches the cell to Excel Value, and it spills as usual, up to the limit on results.

**A table bigger than what the card shows.** Copying and saving take what the card has, and no more:
GridCraft keeps a bounded preview of an object, as a whole one can be any size. The card says so
beforehand and the message after the copy or the save says so again, and the file itself has only those
rows. For the whole table, use Spill Here.

**In a file.** The cell's name and size are its saved value. The table isn't saved, so after opening
the file the card says to run the cell to see it. There is nothing to copy until then.

**A text with line breaks** stays in one cell, under either return type. **Spill One Line per Row** on
the card spills it, with no run: `\n`, `\r\n` and `\r` each end a line, and a break at the very end
starts no extra line. It is one undo step, and it stays with the cell when its code changes. A GridCraft
JSON file keeps it. An xlsx file has no place for it, so there it lasts until the workbook closes: the
file keeps the cells it showed, and when the cell next runs, the text goes back into one cell.

## Plots and pictures

`plt` and `sns` work without an import, and `import matplotlib.pyplot` and `import seaborn` work.
Both need the packages in your Python, which the commands above install. Any picture counts as a
result: a matplotlib figure, a seaborn plot, a pandas plot, a PIL image, or any other object with
`_repr_png_` or `_repr_jpeg_`. A package that draws in a web browser (plotly, bokeh, altair) has no
picture to give, and the editor says so.

**Which result is a picture.** At the end of a cell:

1. If the last expression can give a picture, the cell's result is that picture. This covers what a
   plot call returns: a line, a list of lines, the tuple `plt.hist` gives, an array of axes, a seaborn
   grid.
2. If the last expression gives nothing, and the cell changed pyplot's current figure, the result is
   that figure. This covers an assignment, such as `pairplot = sns.pairplot(df)`, and `plt.show()`.
3. Otherwise the result is the ordinary value. A number or a table comes before an open figure.

One cell gives one picture. An object that can only give SVG or HTML stays an ordinary Python object.

**How it shows.** The cell shows a double square that fills the row and the word `Image`, in the ribbon's
typeface and as large as the cell's text: it takes the width it would have in the cell's own font. Click the double square, or press Ctrl+Shift+F5, and the card opens beside the cell with the picture and
its size. The Python Output menu in the formula bar, or Ctrl+Alt+Shift+M, switches between the two return
types:

- **Python Object:** the cell says `Image`. This is what a picture does until you choose.
- **Excel Value:** the picture is drawn inside the cell, so the cell's size limits it. Widen the column and
  raise the row to see it larger.

A new cell is an Excel Value, so that tables spill. A picture in a cell where nobody chose a type
behaves as a Python Object, and if the code later gives a table, the table spills.

**Display Plot over Cells** (Ctrl+Alt+Shift+C, on the cell's menu, the card and the Python tab) places a
floating picture you can move and resize. It stays tied to its cell:

- When the cell gives a new picture, the floating picture shows it.
- Copy and paste it, or cut and paste it, anywhere in the workbook, or use **Create Reference** beside the
  selected picture: every copy stays tied to the same cell. A floating object can't be pasted into
  another workbook, whichever way it was copied. To take a picture to another application, use Copy Image
  or Save Image on the card.
- The tie follows the cell when rows and columns are inserted, when the cell is cut and pasted, and when
  its sheet is renamed.

**When the pictures go.** A floating picture goes when its cell can no longer give one: when the cell is
cleared, or a value or formula replaces its code, when its row, column or sheet is deleted, or when a run
of its code finishes with something that isn't a picture. Undo brings the code or the cell back, with its
result and every picture, in one step.

A run that doesn't finish deletes nothing. An error, a limit, Stop, or a lost worker leaves the picture
with its last image and a mark, so a typing error doesn't destroy a picture you placed. The mark says which
cell has the problem, and it goes when the cell gives a picture again.

**Resizing.** If the code didn't choose the figure's size, resizing its picture draws the figure again at
the new shape, without running the cell: text keeps its size, and the plot areas stretch. Each copy has
its own shape. With Excel Value, such a figure fills its cell. Below half its natural size it stops being
drawn again, and the picture is scaled, because text would no longer fit.

The code chooses the size with `figsize`, `set_size_inches`, `set_figwidth`, `set_figheight`, `height` or
`aspect` on a seaborn figure-level function, or `figure.figsize` in `rcParams`. Such a figure, and any
other picture, scales as a plain image. Without a worker that has run the cell (Python off, the Enable bar
up, the web), the picture scales until the next run.

**Later cells.** A figure stays open for the cells after it, so `plt.title("...")` in a later cell adds to
it. When a cell runs again, the figures it and later cells made are closed first, so lines don't pile up,
and a cell that changed an earlier cell's figure runs again from that earlier cell. Reset closes every
figure and restores matplotlib's settings.

**Text in other scripts.** The worker adds the system's non-Latin fonts to matplotlib's list, so Chinese,
Japanese, Korean and Thai text draws without any code. `excel.FontPath.CHINESE_SIMPLIFIED` and its
neighbours name a font file, so code written for Excel runs unchanged.

**Limits.** A picture can be 8192 pixels a side, 16 million pixels in all and 16 MiB. A figure past those
gets `plot.tooLarge`, which names its size, before any memory is spent on it. A figure is stored at twice
its own resolution where that fits, so it stays sharp on a dense screen, and the card and `python.image`
report the size at its own resolution, as Excel does. So a figure's file size is about two to four times
what a save at its own resolution gives.

**Start-up time.** The sandbox can't write bytecode, so without compiled bytecode the first plot in every
new worker takes about 10 seconds where it otherwise takes about 0.3. The `uv pip install` lines GridCraft
shows pass `--compile-bytecode` for this reason.

## Initialization code

A workbook can hold code that runs before its Python cells, in the variables they share, as in Excel:
the imports and settings they all rely on. It belongs to no cell. **Initialization**, on the Python
tab and under **Formulas › Python**, opens it in the Python editor window, with the same line numbers,
code checks and problems strip, and the same questions before unsaved code would be lost. Apply is one
undo step, and it is refused while the workbook's structure is protected.

Whenever Python starts for the workbook, this code runs first: at the first cell, after **Reset**,
and after a limit or a crash restarted Python. Changing it, or undoing a change, runs every cell again
from the top in a new Python, so nothing the old code left stays. With no Python cells nothing runs. It
counts toward the limits below as a cell does, and the Run Log lists each run as Initialization.

**When it fails**, every Python cell shows `#PYTHON!` with Python's message and line, and its card
offers **Open Initialization** and **Reset**. Problems lists it once. Nothing runs again until it is
fixed: until the code changes, or you ask with Run Cell, Run All or Reset, or Check Again finds a new
list of packages. **Stop** while it runs isn't a failure: the cells keep what they show, marked stale,
until you ask them to run.

**Code from a file** that a person didn't change (the file doesn't mark it `userModified`) is what
Excel puts in every workbook: imports and settings. A package this Python doesn't have doesn't stop the
cells:

- `import numpy as np`, `import pandas as pd` and `import statsmodels as sm` wait until the name is
  used, as they do without this code. So a workbook doesn't pay for what it doesn't use.
- `import matplotlib.pyplot as plt` and `import seaborn as sns` wait until the name is used, in the
  same way.
- Any other import that fails is skipped, with the lines that use what it would have given, and a
  setting on a module that fails is skipped as well. Excel's own `excel` module, which no other Python
  has, is one of these.
- Problems lists each package it went on without, as `init.skipped`, with the line and the install
  command. A cell that uses such a package fails with its own error.
- Anything else that goes wrong, such as a syntax error or an error in a line that isn't an import or
  a setting, is a failure as above.

Code you change or type here is yours: any error in it is a failure, including a failing import. The
one exception is a line that imports `excel` or uses it, which is skipped and reported, so that a
workbook whose code you changed in Excel still runs.

`xl()` doesn't work in initialization code: there is no cell for it to read for. It holds at most
65,536 characters. A file's code past that is cut with a warning, doesn't run, and is written back cut
by a save; you can shorten it in the editor.

## Saving while cells run

Save waits for the workbook's cells, so the file has their results. While it waits, a bar says
"Waiting for Python cells to finish before saving...", with **Save Now** and **Cancel**, and you can
keep working, in this workbook or another. The save happens as soon as the cells finish.

It waits as long as the cells may take: the time Python may need to start, plus the time limit for each
cell left, plus a second, and never more than two minutes. With the default limits that is 41 seconds
for one cell and two minutes from four cells on. Then it saves what is there and tells you which cells
weren't finished. So does Save Now, and so does a save after **Stop**, after turning Python off, or
after a worker kept going away.

A cell that wasn't finished stores its last result, or none if it never had one, and it runs again the
next time the file is opened with Python on. Its code is always saved. A cell that is stale under manual
calculation doesn't make a save wait.

- **AutoSave** doesn't start a save while cells run. It saves at the next idle moment, and shows no bar.
- **Save As** during a wait doesn't start another one: when the wait ends both files are written, in the
  order you asked for them, and the workbook's file is the last.
- **Close and Quit** during a wait ask as they do for unsaved work. **Save** closes the workbook once it
  has saved, and the workbook you were on before comes back, unless you have gone elsewhere meanwhile.
  **Cancel** returns to the workbook, and the save still comes. If the save fails, or cells weren't
  finished and you didn't press Save Now, the workbook stays open with the reason shown and nothing lost,
  and the quit stops.
- A workbook that closes during its wait isn't saved.

## Reading a saved file with openpyxl

A script outside GridCraft can open a saved workbook with openpyxl. A cell's own Python can't: the
sandbox refuses to open a file.

**What openpyxl can read:**

- **Pictures over the cells.** `ws._images` gives the anchor, the size and the bytes of each. A picture
  linked to a Python cell has the description `gridcraft:python:<n>:<reference>:<alt>`, in
  `image.anchor.pic.nvPicPr.cNvPr.descr`. `<n>` is the length of the reference in characters, because a
  sheet name can hold any separator.
- **The picture of a Python cell, with either return type.** It is in `ws._images` too: hidden
  (`cNvPr.hidden`), anchored on the cell, named `Python G5`, with the description
  `gridcraft:pythonCell:<cell>:<density>:<alt>`. The density is how many of its pixels stand for one of
  the size the card reports, 1 or 2.
- **Python cells.** `load_workbook()` gives the formula, such as `=_xlfn._xlws.PY(0,1)`. With
  `data_only=True`, a Python Object cell whose result is a picture gives the text `Image`.

```python
from openpyxl import load_workbook
from openpyxl.utils import get_column_letter

wb = load_workbook("book.xlsx")
for ws in wb.worksheets:
    for image in ws._images:
        start, props = image.anchor._from, image.anchor.pic.nvPicPr.cNvPr
        print(get_column_letter(start.col + 1), start.row + 1, props.hidden, props.descr)
```

**What can't be done, because of how openpyxl is written:**

- **A cell picture as a cell value.** openpyxl doesn't read rich values, so an Excel Value cell whose
  result is a picture reads as `#VALUE!`. Its picture is in `ws._images`, as above.
- **Python code through openpyxl's interface.** openpyxl doesn't read `xl/python.xml`. It is an ordinary
  part of the package, so a script can read it with `zipfile`:
  `zipfile.ZipFile("book.xlsx").read("xl/python.xml")`.
- **Keeping Python cells through `wb.save()`.** openpyxl removes the parts it doesn't model, so the code
  and the rich values go, and GridCraft can't prevent this. The pictures stay, with their descriptions.
  GridCraft shows them as floating pictures when it opens that file, and warns about the code.

**How the file says so.** A workbook with Python cells has two custom document properties:

- **At load:** a property of type `vt:bstr` whose name is a sentence, which openpyxl prints:
  `UserWarning: Unknown type for GridCraft: this workbook has Python cells. openpyxl cannot read their
  code or in-cell pictures, and wb.save() removes them. See wb.custom_doc_props.` It appears with a plain
  `load_workbook()`, with `read_only=True` and through `pd.read_excel`. `wb.save()` drops this property.
- **On request:** the text property read as `wb.custom_doc_props["GridCraft.Python.Notice"].value`, which
  says where things are. It stays after `wb.save()`, so a later reader learns why the code is gone.

These two aren't your own properties: GridCraft writes them again at each save, and a workbook with no
Python cells loses them. Custom properties you added stay as they are.

**Two limits of openpyxl:**

- It needs Pillow to read pictures. Without Pillow it drops every picture.
- Calling an image's `_data()` closes its stream, so a later `wb.save()` raises `ValueError`. To read the
  bytes and also save, load the workbook twice.

**Opening such a file in GridCraft.** A cell's picture comes back as its result, so a reopened workbook
shows its saved plots without running code, and `python.image` works on it. What another program did to
the file costs no picture:

- A picture saved for a Python cell, with no Python cell at its anchor or with a newer value in that cell,
  shows as a floating picture.
- A linked picture whose source isn't a Python cell keeps its image and loses its link.
- A description that doesn't read as either form is a plain picture's alternative text.

How Excel stores a Python image isn't documented, so these are ordinary pictures with marked descriptions.
How Excel treats them hasn't been tested.

## Limits

- **Time:** 30 seconds per cell by default, from 1 second to 1 hour, plus 10 seconds for the first
  cell while Python starts.
- **Memory:** 1 GB by default, from 128 MB to 256 GB. On macOS this is a check twice a second, not a
  hard cap.
- **Results:** at most 1,000,000 cells, and 128 KiB of text in one value.
- **Printed output:** 1 MiB, then it is cut off.
- **Pictures in a file:** at most 512 MiB in all, counting each cell's. Past that, a cell saves its code
  and the word `Image`, and shows its picture again when it runs.

The time and memory limits are in **File › Options › Python**. At a limit Python is restarted and its
variables are cleared; the next run starts fresh.

## The formula bar

On a Python cell, the PY badge takes the place of fx, and a triangle a line below it runs the cell
(Run Cell). The bar is at least two lines tall there. Pressed while you type code in Python mode, the
triangle enters the code and runs it. It is greyed while the cell runs and where Python can't run,
with the reason when you hover over it. Every other cell has fx.

## The Python tab

The Python tab is contextual, like Table Design. It is there while the active cell is a Python cell,
or while you edit Python code, in the formula bar or the editor window, and it sits at the right end
of the tab bar. Selecting a Python cell adds the tab and leaves the ribbon where it was; click the
tab, or press Alt, Y, to show it. When you leave the cell the tab goes, and if it was showing, the
ribbon returns to Home. A pane you opened from it stays open.

**Formulas › Python** is the way in from anywhere: Insert Python starts Python mode, which brings the
tab up, and Reset, Initialization and Diagnostics are there too. File › Options › Python and the status bar item work
anywhere, and each pane has its own command.

- **Code:** Insert Python, Editor, Initialization, Output As (Python Object or Excel Value).
- **Run:** Run Cell, Run All, Stop, Reset, and Python Calculation (Automatic or Manual; under Manual,
  cells whose inputs changed wait for Run Cell, Run All or F9, and are marked stale).
- **Environment:** a button that shows the state (Off, Not Set Up, Ready, Running, Blocked) and opens
  the Environment pane; Interpreter; Packages; Check Sandbox.
- **Permissions:** What Python Can Do; This Workbook.
- **Diagnostics:** Problems, every cell with something to say, grouped by kind, a click going to the
  cell; Run Log, each run's cell, time and printed output; Clear.

The status bar shows the state when you have Python on or the workbook has Python cells, and while a
cell runs, the cell and how long it has run. A click on it opens the Environment pane.

## When something is wrong

**The PY tag** at the start of a Python cell shows its state, in its shape as well as its colour:

- filled, the plain tag: the result is up to date (a cell that says `Image` has a double square instead);
- filled, with an open ring: it is running or waiting its turn;
- hollow: it keeps a value it didn't just work out, because what it reads changed, it was stopped, or
  Python isn't on;
- red, with an exclamation mark: it failed;
- grey, with a lock: Python can't run here.

**The card** opens from the tag or the double square, or with Ctrl+Shift+F5. It says what happened in one sentence, then
Python's own message with its line, and has a button for what to do: Open Settings, Choose
Interpreter, Copy Install Command, Enable, Run Again, Open Initialization with Reset, or Go To Cell. A failed run that printed
something has Show Printed Output. A result keeps its type and preview, or its table
([Python objects and tables](#python-objects-and-tables)). Copy Install Command gives the uv command;
pip is behind **Other package managers**.

**In the editors**, the formula bar in Python mode and the Python editor window check your code as
you type, with Python off too and on systems with no sandbox. They need nothing to run:

- a wavy underline on what the sandbox will refuse (a file, the network, another program, a package that draws in a browser, an
  `xl()` whose reference is computed) and on a package this Python doesn't have, with the reason when
  you hover over it;
- a chip with Python's state, which opens the Environment pane;
- in the editor window, a mark beside each such line, and a strip under the code that lists these
  and what the last run said. A click on a row goes to its line. The last run's line is marked only
  while the code is what ran.

A package is only called missing once GridCraft knows what this Python has.

**Problems** on the Python tab also lists, for the whole workbook, each package the code imports that
this Python doesn't have, with its install command, and says when pandas is missing, which makes
ranges come back as lists of rows.

## For agents

Agents can read all of this and run cells, once a person has turned Python on: `python.environment`,
`python.diagnostics`, `python.runLog`, `python.run`, `python.runAll`, `python.initialization`,
`ui.pythonInitialization`, `ui.inspect`. `file.save` with `wait` holds its reply until the save has
happened, and the reply says which cells weren't finished; `engine.execute` with `wait` holds it until
Python is idle. See [control-protocol.md](control-protocol.md#saving-and-waiting-for-python-cells).
`python.table`, `python.saveCsv`, `ui.pythonInitialization`, `ui.inspect`. See
[control-protocol.md](control-protocol.md). For pictures: `python.image` gives a cell's picture as base64
with its size, `python.saveImage` writes it to a path, `python.output` sets the return type,
`python.displayOverCells` and `python.createReference` make linked pictures, and `object.move` resizes
one, which draws a free figure again. None of them opens a dialog.
