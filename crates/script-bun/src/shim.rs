//! The Office Scripts shim, embedded as a string: the JavaScript/TypeScript side of the bridge.
//!
//! It defines the documented `ExcelScript` subset (`workbook`, `getActiveWorksheet`,
//! `getWorksheet`, `getRange`, `setValue`, `setValues`, `setFormula`, `setFormulas`,
//! `setNumberFormat`, `getFormat().getFont().setBold`, `addWorksheet`, `delete`, `console.log`)
//! and forwards every workbook operation to GridCraft over stdin/stdout. It never edits a
//! workbook itself. The maintainer decision that allows this one script is recorded in
//! `docs/decisions.md`; it is runtime plumbing, not application logic.
//!
//! The file is written to a temporary directory for the duration of the host and removed with it.

use gridcraft_script::ScriptError;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The shim source. It is a `&str` on purpose: nothing in the build transpiles or bundles it.
pub(crate) const BOOTSTRAP: &str = r##"import { readSync, writeSync } from "node:fs";

const decoder = new TextDecoder();
const encoder = new TextEncoder();
let input = "";

function readLine(): string | null {
  for (;;) {
    const newline = input.indexOf("\n");
    if (newline >= 0) {
      const line = input.slice(0, newline);
      input = input.slice(newline + 1);
      return line;
    }
    const chunk = Buffer.alloc(65536);
    let read = 0;
    try {
      read = readSync(0, chunk, 0, chunk.length, null);
    } catch {
      return null;
    }
    if (read <= 0) return null;
    input += decoder.decode(chunk.subarray(0, read));
  }
}

function writeAll(fd: number, text: string): void {
  const bytes = encoder.encode(text);
  let offset = 0;
  while (offset < bytes.length) {
    offset += writeSync(fd, bytes, offset, bytes.length - offset);
  }
}

function writeLine(text: string): void {
  writeAll(1, text + "\n");
}

class OfficeScriptsError extends Error {
  code: string;
  member?: string;
  constructor(message: string, code: string = "notSupported", member?: string) {
    super(message);
    this.name = "OfficeScriptsError";
    this.code = code;
    this.member = member;
  }
}

function notSupported(member: string): OfficeScriptsError {
  return new OfficeScriptsError(
    member + " is not supported by GridCraft's Office Scripts bridge",
    "notSupported",
    member,
  );
}

let nextId = 1;

function bridge(method: string, params: any): any {
  const id = nextId++;
  writeLine(JSON.stringify({ id, method, params }));
  const reply = readLine();
  if (reply === null) throw new Error("GridCraft closed the script bridge");
  let parsed: any;
  try {
    parsed = JSON.parse(reply);
  } catch {
    throw new Error("GridCraft sent a reply the script could not read");
  }
  if (parsed.error) {
    throw new OfficeScriptsError(
      String(parsed.error.message ?? "the workbook refused the call"),
      String(parsed.error.code ?? "engine"),
    );
  }
  return parsed.result;
}

/// Unknown members throw a typed error naming the class and member instead of a bare TypeError.
function guarded<T extends object>(className: string, target: T): T {
  return new Proxy(target, {
    get(t, prop, receiver) {
      if (typeof prop === "symbol") return Reflect.get(t, prop, receiver);
      if (prop in t) return Reflect.get(t, prop, receiver);
      throw notSupported(className + "." + String(prop));
    },
  });
}

function columnName(column: number): string {
  if (!Number.isInteger(column) || column < 0) throw new Error("a column number is a whole number from 0");
  let name = "";
  let n = column + 1;
  while (n > 0) {
    const rest = (n - 1) % 26;
    name = String.fromCharCode(65 + rest) + name;
    n = Math.floor((n - 1) / 26);
  }
  return name;
}

function columnIndex(letters: string): number {
  let n = 0;
  for (const ch of letters.toUpperCase()) {
    const v = ch.charCodeAt(0);
    if (v < 65 || v > 90) throw new Error("bad column " + letters);
    n = n * 26 + (v - 64);
  }
  return n - 1;
}

interface Address {
  r0: number;
  c0: number;
  r1: number;
  c1: number;
}

const CELL = /^\$?([A-Za-z]{1,3})\$?(\d+)$/;
const COLS = /^\$?([A-Za-z]{1,3}):\$?([A-Za-z]{1,3})$/;
const ROWS = /^\$?(\d+):\$?(\d+)$/;

function parseAddress(address: string): Address {
  const text = String(address ?? "").trim();
  if (text === "") throw new Error('a range needs an address such as "A1:B2"');
  const parts = text.split(":");
  const one = (s: string): { r: number; c: number } | null => {
    const m = CELL.exec(s);
    if (!m) return null;
    const row = Number(m[2]);
    if (!Number.isInteger(row) || row < 1) return null;
    return { r: row - 1, c: columnIndex(m[1]) };
  };
  const a = one(parts[0]);
  const b = parts.length > 1 ? one(parts[1]) : a;
  if (a && b) return { r0: Math.min(a.r, b.r), c0: Math.min(a.c, b.c), r1: Math.max(a.r, b.r), c1: Math.max(a.c, b.c) };
  const cols = COLS.exec(text);
  if (cols) {
    const c0 = columnIndex(cols[1]);
    const c1 = columnIndex(cols[2]);
    return { r0: 0, c0: Math.min(c0, c1), r1: 1048575, c1: Math.max(c0, c1) };
  }
  const rows = ROWS.exec(text);
  if (rows) {
    const r0 = Number(rows[1]);
    const r1 = Number(rows[2]);
    if (r0 >= 1 && r1 >= 1) return { r0: Math.min(r0, r1) - 1, c0: 0, r1: Math.max(r0, r1) - 1, c1: 16383 };
  }
  throw new Error("bad range address " + text);
}

function addressOf(a: Address): string {
  if (a.r0 === 0 && a.r1 === 1048575 && !(a.c0 === 0 && a.c1 === 16383)) return columnName(a.c0) + ":" + columnName(a.c1);
  if (a.c0 === 0 && a.c1 === 16383 && !(a.r0 === 0 && a.r1 === 1048575)) return String(a.r0 + 1) + ":" + String(a.r1 + 1);
  const start = columnName(a.c0) + String(a.r0 + 1);
  const end = columnName(a.c1) + String(a.r1 + 1);
  return start === end ? start : start + ":" + end;
}

function sheetParams(name: string | undefined): any {
  return name === undefined ? {} : { sheet: name };
}

class Worksheet {
  constructor(public sheetName?: string) {
    return guarded("Worksheet", this);
  }
  getRange(address: string): Range {
    return new Range(addressOf(parseAddress(address)), this.sheetName);
  }
  getCell(row: number, column: number): Range {
    if (!Number.isInteger(row) || !Number.isInteger(column) || row < 0 || column < 0) {
      throw new Error("getCell takes non-negative whole row and column numbers");
    }
    return new Range(columnName(column) + String(row + 1), this.sheetName);
  }
  getRangeByIndexes(startRow: number, startColumn: number, rowCount: number, columnCount: number): Range {
    const r0 = startRow;
    const c0 = startColumn;
    const a: Address = { r0, c0, r1: r0 + Math.max(0, rowCount - 1), c1: c0 + Math.max(0, columnCount - 1) };
    return new Range(addressOf(a), this.sheetName);
  }
  getUsedRange(): Range {
    const info = bridge("Worksheet.getUsedRange", sheetParams(this.sheetName));
    return new Range(String(info.address), this.sheetName);
  }
  getName(): string {
    return String(bridge("Worksheet.getName", sheetParams(this.sheetName)).sheet ?? "");
  }
  setName(name: string): void {
    bridge("Worksheet.setName", { ...sheetParams(this.sheetName), name });
    this.sheetName = name;
  }
  activate(): void {
    bridge("Worksheet.activate", sheetParams(this.sheetName));
    this.sheetName = this.getName();
  }
  delete(): void {
    bridge("Worksheet.delete", sheetParams(this.sheetName));
  }
  getStandardHeight(): number {
    return Number(bridge("Worksheet.getStandardHeight", sheetParams(this.sheetName)));
  }
  getStandardWidth(): number {
    return Number(bridge("Worksheet.getStandardWidth", sheetParams(this.sheetName)));
  }
  getTables(): Table[] {
    return (bridge("Worksheet.getTables", sheetParams(this.sheetName)) as any[]).map((t) => new Table(String(t.name)));
  }
  getCharts(): Chart[] {
    return (bridge("Worksheet.getCharts", sheetParams(this.sheetName)) as any[]).map((c) => new Chart(Number(c.id)));
  }
  getIndex(): number {
    return Number(bridge("Worksheet.getIndex", sheetParams(this.sheetName)));
  }
  getVisibility(): string {
    return String(bridge("Worksheet.getVisibility", sheetParams(this.sheetName)));
  }
  setVisibility(visibility: string): void {
    bridge("Worksheet.setVisibility", { ...sheetParams(this.sheetName), visibility });
  }
  getVisible(): boolean {
    return this.getVisibility() === "visible";
  }
  setVisible(visible: boolean): void {
    this.setVisibility(visible ? "visible" : "hidden");
  }
}

class Border {
  constructor(
    private rangeAddress: Range,
    public side: string,
  ) {
    return guarded("RangeBorder", this);
  }
  params(extra: any): any {
    return this.rangeAddress.params({ side: this.side, ...extra });
  }
  getColor(): string {
    return String(bridge("RangeBorder.getColor", this.params({})) ?? "");
  }
  setColor(color: string): void {
    bridge("RangeBorder.setColor", this.params({ color }));
  }
  getStyle(): string {
    return String(bridge("RangeBorder.getStyle", this.params({})) ?? "continuous");
  }
  setStyle(style: string): void {
    bridge("RangeBorder.setStyle", this.params({ style }));
  }
  getWeight(): string {
    return String(bridge("RangeBorder.getWeight", this.params({})) ?? "thin");
  }
  setWeight(weight: string): void {
    bridge("RangeBorder.setWeight", this.params({ weight }));
  }
  getSideIndex(): string {
    return this.side;
  }
}

class Borders {
  constructor(private rangeAddress: Range) {
    return guarded("RangeBorders", this);
  }
  getItem(side: string): Border {
    return new Border(this.rangeAddress, side);
  }
}

class Font {
  constructor(private rangeAddress: Range) {
    return guarded("RangeFont", this);
  }
  params(extra: any): any {
    return this.rangeAddress.params(extra);
  }
  getBold(): boolean {
    return Boolean(bridge("RangeFont.getBold", this.params({})));
  }
  setBold(bold: boolean): void {
    bridge("RangeFont.setBold", this.params({ bold: !!bold }));
  }
  getItalic(): boolean {
    return Boolean(bridge("RangeFont.getItalic", this.params({})));
  }
  setItalic(italic: boolean): void {
    bridge("RangeFont.setItalic", this.params({ italic: !!italic }));
  }
  getUnderline(): string {
    return String(bridge("RangeFont.getUnderline", this.params({})) ?? "none");
  }
  setUnderline(style: string): void {
    bridge("RangeFont.setUnderline", this.params({ underline: style }));
  }
  getSize(): number {
    return Number(bridge("RangeFont.getSize", this.params({})));
  }
  setSize(size: number): void {
    bridge("RangeFont.setSize", this.params({ size }));
  }
  getName(): string {
    return String(bridge("RangeFont.getName", this.params({})) ?? "");
  }
  setName(name: string): void {
    bridge("RangeFont.setName", this.params({ name }));
  }
  getColor(): string {
    return String(bridge("RangeFont.getColor", this.params({})) ?? "");
  }
  setColor(color: string): void {
    bridge("RangeFont.setColor", this.params({ color }));
  }
  getStrikethrough(): boolean {
    return Boolean(bridge("RangeFont.getStrikethrough", this.params({})));
  }
}

class Fill {
  constructor(private rangeAddress: Range) {
    return guarded("RangeFill", this);
  }
  getColor(): string {
    return String(bridge("RangeFill.getColor", this.rangeAddress.params({})) ?? "");
  }
  setColor(color: string): void {
    bridge("RangeFill.setColor", this.rangeAddress.params({ color }));
  }
}

class Format {
  constructor(private rangeAddress: Range) {
    return guarded("RangeFormat", this);
  }
  getFont(): Font {
    return new Font(this.rangeAddress);
  }
  getFill(): Fill {
    return new Fill(this.rangeAddress);
  }
  getBorders(): Borders {
    return new Borders(this.rangeAddress);
  }
  getRangeBorder(index: string): Border {
    return new Border(this.rangeAddress, index);
  }
  getNumberFormat(): string {
    return String(bridge("RangeFormat.getNumberFormat", this.rangeAddress.params({})) ?? "General");
  }
  setNumberFormat(format: string): void {
    bridge("RangeFormat.setNumberFormat", this.rangeAddress.params({ format }));
  }
  getHorizontalAlignment(): string {
    return String(bridge("RangeFormat.getHorizontalAlignment", this.rangeAddress.params({})) ?? "General");
  }
  getVerticalAlignment(): string {
    return String(bridge("RangeFormat.getVerticalAlignment", this.rangeAddress.params({})) ?? "Bottom");
  }
}

class Comment {
  constructor(
    public sheetName: string | undefined,
    public cell: string,
  ) {
    return guarded("Comment", this);
  }
  params(extra: any): any {
    return { ...sheetParams(this.sheetName), cell: this.cell, ...extra };
  }
  getContent(): string {
    return String(bridge("Comment.getContent", this.params({})) ?? "");
  }
  setContent(text: string): void {
    bridge("Comment.setContent", this.params({ text }));
  }
  getAuthorName(): string {
    return String(bridge("Comment.getAuthorName", this.params({})) ?? "");
  }
  getVisible(): boolean {
    return Boolean(bridge("Comment.getVisible", this.params({})));
  }
  delete(): void {
    bridge("Comment.delete", this.params({}));
  }
}

class DataValidation {
  constructor(
    public sheetName: string | undefined,
    public cell: string,
  ) {
    return guarded("DataValidation", this);
  }
  params(extra: any): any {
    return { ...sheetParams(this.sheetName), cell: this.cell, ...extra };
  }
  getType(): string {
    return String(bridge("DataValidation.getType", this.params({})) ?? "none");
  }
  getRule(): any {
    return bridge("DataValidation.getRule", this.params({}));
  }
  setRule(rule: any): void {
    bridge("DataValidation.setRule", this.params({ rule }));
  }
  getErrorAlert(): any {
    return bridge("DataValidation.getErrorAlert", this.params({}));
  }
  setErrorAlert(alert: any): void {
    bridge("DataValidation.setErrorAlert", this.params({ alert }));
  }
  getPrompt(): any {
    return bridge("DataValidation.getPrompt", this.params({}));
  }
  setPrompt(prompt: any): void {
    bridge("DataValidation.setPrompt", this.params({ prompt }));
  }
  getIgnoreBlanks(): boolean {
    return Boolean(bridge("DataValidation.getIgnoreBlanks", this.params({})));
  }
  setIgnoreBlanks(ignoreBlanks: boolean): void {
    bridge("DataValidation.setIgnoreBlanks", this.params({ ignoreBlanks: !!ignoreBlanks }));
  }
  delete(): void {
    bridge("DataValidation.clear", this.params({}));
  }
}

class Range {
  constructor(
    public address: string,
    public sheetName?: string,
  ) {
    return guarded("Range", this);
  }
  params(extra: any): any {
    const params: any = { range: this.address, ...sheetParams(this.sheetName) };
    for (const key of Object.keys(extra)) params[key] = extra[key];
    return params;
  }
  getAddress(): string {
    return this.address;
  }
  getAddressLocal(): string {
    return this.address;
  }
  getWorksheet(): Worksheet {
    const info = bridge("Range.getWorksheet", this.params({}));
    return new Worksheet(String(info.sheet));
  }
  getFormat(): Format {
    return new Format(this);
  }
  getRowCount(): number {
    const a = parseAddress(this.address);
    return a.r1 - a.r0 + 1;
  }
  getColumnCount(): number {
    const a = parseAddress(this.address);
    return a.c1 - a.c0 + 1;
  }
  getRowIndex(): number {
    return parseAddress(this.address).r0;
  }
  getColumnIndex(): number {
    return parseAddress(this.address).c0;
  }
  getValue(): any {
    return bridge("Range.getValue", this.params({}));
  }
  getValues(): any[][] {
    return bridge("Range.getValues", this.params({}));
  }
  getText(): string {
    return String(bridge("Range.getText", this.params({})) ?? "");
  }
  getTexts(): string[][] {
    return bridge("Range.getTexts", this.params({}));
  }
  getFormula(): string {
    return String(bridge("Range.getFormula", this.params({})) ?? "");
  }
  getFormulas(): string[][] {
    return bridge("Range.getFormulas", this.params({}));
  }
  getNumberFormat(): string {
    return String(bridge("Range.getNumberFormat", this.params({})) ?? "General");
  }
  getNumberFormats(): string[][] {
    return bridge("Range.getNumberFormats", this.params({}));
  }
  setValue(value: any): void {
    bridge("Range.setValue", this.params({ value }));
  }
  setValues(values: any[][]): void {
    bridge("Range.setValues", this.params({ values }));
  }
  setText(text: string): void {
    bridge("Range.setText", this.params({ text }));
  }
  setTexts(texts: string[][]): void {
    bridge("Range.setTexts", this.params({ texts }));
  }
  setFormula(formula: string): void {
    bridge("Range.setFormula", this.params({ formula }));
  }
  setFormulas(formulas: string[][]): void {
    bridge("Range.setFormulas", this.params({ formulas }));
  }
  setNumberFormat(format: string): void {
    bridge("Range.setNumberFormat", this.params({ format }));
  }
  setNumberFormats(formats: string[][]): void {
    bridge("Range.setNumberFormats", this.params({ formats }));
  }
  getCell(row: number, column: number): Range {
    const a = parseAddress(this.address);
    if (!Number.isInteger(row) || !Number.isInteger(column) || row < 0 || column < 0) {
      throw new Error("getCell takes non-negative whole row and column numbers");
    }
    return new Range(columnName(a.c0 + column) + String(a.r0 + row + 1), this.sheetName);
  }
  getOffsetRange(rowOffset: number, columnOffset: number): Range {
    const a = parseAddress(this.address);
    return new Range(
      addressOf({ r0: a.r0 + rowOffset, c0: a.c0 + columnOffset, r1: a.r1 + rowOffset, c1: a.c1 + columnOffset }),
      this.sheetName,
    );
  }
  getEntireRow(): Range {
    const a = parseAddress(this.address);
    return new Range(String(a.r0 + 1) + ":" + String(a.r1 + 1), this.sheetName);
  }
  getEntireColumn(): Range {
    const a = parseAddress(this.address);
    return new Range(columnName(a.c0) + ":" + columnName(a.c1), this.sheetName);
  }
  getBoundingRect(anotherRange: Range): Range {
    if (anotherRange.sheetName !== this.sheetName) throw new Error("getBoundingRect needs both ranges on one worksheet");
    const a = parseAddress(this.address);
    const b = parseAddress(anotherRange.address);
    return new Range(
      addressOf({
        r0: Math.min(a.r0, b.r0),
        c0: Math.min(a.c0, b.c0),
        r1: Math.max(a.r1, b.r1),
        c1: Math.max(a.c1, b.c1),
      }),
      this.sheetName,
    );
  }
  getRangeBetween(anotherRange: Range): Range {
    return this.getBoundingRect(anotherRange);
  }
  getTables(): Table[] {
    return (bridge("Range.getTables", this.params({})) as any[]).map((t) => new Table(String(t.name)));
  }
  getDataValidation(): DataValidation | undefined {
    const info = bridge("Range.getDataValidation", this.params({}));
    if (!info) return undefined;
    return new DataValidation(this.sheetName, String(info.cell));
  }
  getComment(): Comment | undefined {
    const info = bridge("Range.getComment", this.params({}));
    if (!info) return undefined;
    return new Comment(this.sheetName, String(info.cell));
  }
  insert(shift?: string): void {
    bridge("Range.insert", this.params({ shift: shift ?? "Down" }));
  }
  delete(shift?: string): void {
    bridge("Range.delete", this.params({ shift: shift ?? "Up" }));
  }
  clear(applyTo?: string): void {
    bridge("Range.clear", this.params({ applyTo: applyTo ?? "All" }));
  }
  merge(): void {
    bridge("Range.merge", this.params({}));
  }
  unmerge(): void {
    bridge("Range.unmerge", this.params({}));
  }
  select(): void {
    bridge("Range.select", this.params({}));
  }
}

class RangeAreas {
  constructor(
    public areas: string[],
    public sheetName?: string,
  ) {
    return guarded("RangeAreas", this);
  }
  params(extra: any): any {
    return { areas: this.areas, ...sheetParams(this.sheetName), ...extra };
  }
  getAreas(): Range[] {
    const list = bridge("RangeAreas.getAreas", this.params({})) as any[];
    return list.map((a) => new Range(String(a.address), this.sheetName));
  }
  getAddress(): string {
    return String(bridge("RangeAreas.getAddress", this.params({})));
  }
  getFormat(): Format {
    const first = this.areas[0] ?? "A1";
    return new Format(new Range(first, this.sheetName));
  }
}

class TableColumn {
  constructor(
    public tableName: string,
    public columnName: string,
    public index: number,
  ) {
    return guarded("TableColumn", this);
  }
  params(extra: any): any {
    return { table: this.tableName, column: this.columnName, ...extra };
  }
  getRange(): Range {
    const info = bridge("TableColumn.getRange", this.params({}));
    return new Range(String(info.address), String(info.sheet));
  }
  getHeaderRowRange(): Range {
    const info = bridge("TableColumn.getHeaderRowRange", this.params({}));
    return new Range(String(info.address), String(info.sheet));
  }
  getDataBodyRange(): Range {
    const info = bridge("TableColumn.getDataBodyRange", this.params({}));
    return new Range(String(info.address), String(info.sheet));
  }
  getTotalsRowRange(): Range {
    const info = bridge("TableColumn.getTotalRowRange", this.params({}));
    return new Range(String(info.address), String(info.sheet));
  }
  getName(): string {
    return String(bridge("TableColumn.getName", this.params({})) ?? this.columnName);
  }
  getIndex(): number {
    return Number(bridge("TableColumn.getIndex", this.params({})));
  }
}

class Table {
  constructor(public tableName: string) {
    return guarded("Table", this);
  }
  params(extra: any): any {
    return { table: this.tableName, ...extra };
  }
  getRange(): Range {
    const info = bridge("Table.getRange", this.params({}));
    return new Range(String(info.address), String(info.sheet));
  }
  getHeaderRowRange(): Range {
    const info = bridge("Table.getHeaderRowRange", this.params({}));
    return new Range(String(info.address), String(info.sheet));
  }
  getDataBodyRange(): Range {
    const info = bridge("Table.getDataBodyRange", this.params({}));
    return new Range(String(info.address), String(info.sheet));
  }
  getTotalsRowRange(): Range {
    const info = bridge("Table.getTotalRowRange", this.params({}));
    return new Range(String(info.address), String(info.sheet));
  }
  getName(): string {
    return String(bridge("Table.getName", this.params({})) ?? this.tableName);
  }
  setName(name: string): void {
    bridge("Table.setName", this.params({ name }));
    this.tableName = name;
  }
  getColumns(): TableColumn[] {
    const list = bridge("Table.getColumns", this.params({})) as any[];
    return list.map((c) => new TableColumn(this.tableName, String(c.name), Number(c.index)));
  }
  getColumn(name: string): TableColumn {
    const info = bridge("Table.getColumn", this.params({ name }));
    return new TableColumn(this.tableName, String(info.name), Number(info.index));
  }
  getStyle(): string {
    return String(bridge("Table.getPredefinedTableStyle", this.params({})) ?? "");
  }
  setStyle(style: string): void {
    bridge("Table.setPredefinedTableStyle", this.params({ style }));
  }
  getShowHeaders(): boolean {
    return Boolean(bridge("Table.getShowHeaders", this.params({})));
  }
  setShowHeaders(on: boolean): void {
    bridge("Table.setShowHeaders", this.params({ on: !!on }));
  }
  getShowTotals(): boolean {
    return Boolean(bridge("Table.getShowTotals", this.params({})));
  }
  setShowTotals(on: boolean): void {
    bridge("Table.setShowTotals", this.params({ on: !!on }));
  }
  convertToRange(): void {
    bridge("Table.convertToRange", this.params({}));
  }
  delete(): void {
    bridge("Table.delete", this.params({}));
  }
}

class Chart {
  constructor(public chartId: number) {
    return guarded("Chart", this);
  }
  params(extra: any): any {
    return { id: this.chartId, ...extra };
  }
  getChartType(): string {
    return String(bridge("Chart.getChartType", this.params({})) ?? "columnClustered");
  }
  setChartType(type: string): void {
    bridge("Chart.setChartType", this.params({ type }));
  }
  getRange(): string {
    return String(bridge("Chart.getRange", this.params({})) ?? "");
  }
  getName(): string {
    return String(bridge("Chart.getName", this.params({})) ?? "");
  }
  setName(name: string): void {
    bridge("Chart.setName", this.params({ name }));
  }
  getLeft(): number {
    return Number(bridge("Chart.getLeft", this.params({})));
  }
  setLeft(value: number): void {
    bridge("Chart.setLeft", this.params({ value }));
  }
  getTop(): number {
    return Number(bridge("Chart.getTop", this.params({})));
  }
  setTop(value: number): void {
    bridge("Chart.setTop", this.params({ value }));
  }
  getWidth(): number {
    return Number(bridge("Chart.getWidth", this.params({})));
  }
  setWidth(value: number): void {
    bridge("Chart.setWidth", this.params({ value }));
  }
  getHeight(): number {
    return Number(bridge("Chart.getHeight", this.params({})));
  }
  setHeight(value: number): void {
    bridge("Chart.setHeight", this.params({ value }));
  }
  setPosition(left: number, top: number, width: number, height: number): void {
    bridge("Chart.setPosition", this.params({ left, top, width, height }));
  }
  delete(): void {
    bridge("Chart.delete", this.params({}));
  }
}

class NamedItem {
  constructor(public itemName: string) {
    return guarded("NamedItem", this);
  }
  params(extra: any): any {
    return { name: this.itemName, ...extra };
  }
  getName(): string {
    return String(bridge("NamedItem.getName", this.params({})) ?? this.itemName);
  }
  getRefersTo(): string {
    return String(bridge("NamedItem.getFormula", this.params({})) ?? "");
  }
  getScope(): string {
    return String(bridge("NamedItem.getScope", this.params({})) ?? "");
  }
  getComment(): string {
    return String(bridge("NamedItem.getComment", this.params({})) ?? "");
  }
  getVisible(): boolean {
    return Boolean(bridge("NamedItem.getVisible", this.params({})));
  }
  getType(): string {
    return String(bridge("NamedItem.getType", this.params({})) ?? "Range");
  }
}

class Application {
  constructor(public appName: string, public appVersion: string) {
    return guarded("Application", this);
  }
  getName(): string {
    return this.appName;
  }
  getVersion(): string {
    return this.appVersion;
  }
}

class Workbook {
  constructor() {
    return guarded("Workbook", this);
  }
  getActiveWorksheet(): Worksheet {
    const info = bridge("Workbook.getActiveWorksheet", {});
    return new Worksheet(String(info.sheet));
  }
  getWorksheets(): Worksheet[] {
    return (bridge("Workbook.getWorksheets", {}) as any[]).map((s) => new Worksheet(String(s.name)));
  }
  getWorksheet(name: string): Worksheet {
    return new Worksheet(String(name));
  }
  getActiveCell(): Range {
    const info = bridge("Workbook.getActiveCell", {});
    return new Range(String(info.address), String(info.sheet));
  }
  getSelectedRange(): Range {
    const info = bridge("Workbook.getSelectedRange", {});
    return new Range(String(info.address), String(info.sheet));
  }
  getSelectedRangeAreas(): RangeAreas {
    const info = bridge("Workbook.getSelectedRanges", {});
    const areas = (info.areas as any[]).map((a) => String(a.address));
    return new RangeAreas(areas, String(info.sheet));
  }
  getName(): string {
    return String(bridge("Workbook.getName", {}));
  }
  getIsDirty(): boolean {
    return Boolean(bridge("Workbook.getIsDirty", {}));
  }
  getApplication(): Application {
    const info = bridge("Workbook.getApplication", {});
    return new Application(String(info.name), String(info.version));
  }
  addWorksheet(name?: string, position?: number): Worksheet {
    const info = bridge("Workbook.addWorksheet", name === undefined ? {} : { name, position });
    return new Worksheet(String(info.sheet));
  }
  getNames(): NamedItem[] {
    return (bridge("Workbook.getNames", {}) as any[]).map((n) => new NamedItem(String(n.name)));
  }
  getNamedItem(name: string): NamedItem | undefined {
    const info = bridge("Workbook.getNamedItem", { name });
    return info ? new NamedItem(String(info.name)) : undefined;
  }
}

const workbook = new Workbook();

function describeValue(value: any): string {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

const consoleProxy = {
  log: (...args: any[]): void => {
    bridge("console.log", { message: args.map(describeValue).join(" ") });
  },
  info: (...args: any[]): void => {
    bridge("console.log", { message: args.map(describeValue).join(" ") });
  },
  warn: (...args: any[]): void => {
    bridge("console.log", { message: args.map(describeValue).join(" ") });
  },
  error: (...args: any[]): void => {
    bridge("console.log", { message: args.map(describeValue).join(" ") });
  },
};

function describe(error: any): any {
  if (error && Array.isArray(error.errors) && error.errors.length > 0) {
    const first = error.errors[0];
    const line = first.position && typeof first.position.line === "number" ? first.position.line + 1 : undefined;
    return { message: String(first.message ?? error.message ?? error), line };
  }
  const message = String(error && error.message ? error.message : error);
  const stack = error && error.stack ? String(error.stack) : "";
  let line: number | undefined = undefined;
  const found = /<anonymous>:(\d+):(\d+)/.exec(stack);
  if (found) {
    const n = Number(found[1]) - 2;
    if (n >= 1) line = n;
  }
  return { message, line };
}

let failed: any = null;
try {
  const initLine = readLine();
  if (initLine === null) {
    writeLine(JSON.stringify({ fatal: { message: "GridCraft did not send a script" } }));
    process.exit(2);
  }
  const init = JSON.parse(initLine);
  const transpiler = new Bun.Transpiler({ loader: "ts" });
  const js = transpiler.transformSync(String(init.script ?? ""));
  const factory = new Function(
    "workbook",
    "console",
    js + "\nreturn typeof main === 'function' ? main : undefined;",
  );
  const entry = factory(workbook, consoleProxy);
  if (typeof entry !== "function") {
    throw new Error("an Office Script needs a main function: function main(workbook: ExcelScript.Workbook) { ... }");
  }
  entry(workbook);
} catch (error: any) {
  failed = describe(error);
}

if (failed) {
  writeLine(JSON.stringify({ fatal: failed }));
  process.exit(1);
}
"##;

/// New temp shim names.
static SHIM_SEQ: AtomicU64 = AtomicU64::new(1);

/// The shim's file on disk, removed when the host is dropped.
pub(crate) struct TempShim {
    path: PathBuf,
}

impl TempShim {
    pub(crate) fn write() -> Result<TempShim, ScriptError> {
        let unique = (std::process::id() as u64).saturating_mul(1000).saturating_add(SHIM_SEQ.fetch_add(1, Ordering::Relaxed));
        let path = std::env::temp_dir().join(format!("gridcraft-office-scripts-{unique}.ts"));
        std::fs::write(&path, BOOTSTRAP).map_err(|e| ScriptError::runtime(format!("the Office Scripts bridge could not be written: {e}")))?;
        Ok(TempShim { path })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempShim {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
