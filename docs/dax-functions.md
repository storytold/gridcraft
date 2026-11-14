# DAX functions implemented in GridCraft

This list is generated from the `gridcraft-dax` catalog and evaluator. **Parsed** means the
function is in the catalog with arity/parameter validation and round-trips through the parser
and printer. **Evaluated** means the evaluator has a dispatch path for it.

- Catalogued (parsed and validated): **484**
- Evaluated by the scalar core in this crate: **the aggregations, the logical spine and the
  relationship/table helpers listed in `crates/dax/README.md`**
- Every other catalogued function, including the ones named below that the scalar core does
  not carry: **not implemented until its evaluator family is linked**

The per-category counts below are the catalog's. This crate links no evaluator family yet
(`eval::FAMILIES` is empty), so a catalogued function outside the scalar core returns
`not implemented`; each family PR adds its own dispatch and its own suite.

| Category | Catalogued |
| --- | ---: |
| Aggregation | 22 |
| Date and time | 24 |
| Filter | 29 |
| Financial | 51 |
| Information | 114 |
| Logical | 13 |
| Math and trig | 49 |
| Other | 16 |
| Parent and child | 5 |
| Relationships | 4 |
| Statistical | 43 |
| Table manipulation | 35 |
| Text | 23 |
| Time intelligence | 45 |
| Visual calculations | 11 |

## Evaluated functions

In this crate the evaluator is `Evaluator::eval`'s built-in switch plus whatever `eval::FAMILIES`
links, and `FAMILIES` is empty here. The switch handles the aggregations and their `*X` iterators,
the logical spine, `CALCULATE`/`FILTER`/`ALL`/`ALLEXCEPT`/`ALLNOBLANKROW`/`ALLSELECTED`/`VALUES`/`DISTINCT`/`SELECTEDVALUE`, `RELATED*`,
`COUNTROWS`, the statement operators, and the scalar helpers it carries directly — `ABS`, `DATE`,
`LEN`, `PATHLENGTH` and the rest of the small scalar set in `crates/dax/src/eval.rs`. Every other
catalogued function reports "not implemented" until its evaluator family is linked; each family PR
adds its dispatch and its own suite under `crates/dax/tests/`. The lists below are therefore the
**catalog**, not a claim that this build evaluates every name in it.

## Catalogued (parsed and validated)

### Aggregation

`APPROXIMATEDISTINCTCOUNT`, `AVERAGE`, `AVERAGEA`, `AVERAGEX`, `COUNT`, `COUNTA`, `COUNTAX`, `COUNTBLANK`, `COUNTROWS`, `COUNTX`, `DISTINCTCOUNT`, `DISTINCTCOUNTNOBLANK`, `MAX`, `MAXA`, `MAXX`, `MIN`, `MINA`, `MINX`, `PRODUCT`, `PRODUCTX`, `SUM`, `SUMX`

### Date and time

`CALENDAR`, `CALENDARAUTO`, `DATE`, `DATEDIFF`, `DATEVALUE`, `DAY`, `EDATE`, `EOMONTH`, `HOUR`, `MINUTE`, `MONTH`, `NETWORKDAYS`, `NOW`, `QUARTER`, `SECOND`, `TIME`, `TIMEVALUE`, `TODAY`, `UTCNOW`, `UTCTODAY`, `WEEKDAY`, `WEEKNUM`, `YEAR`, `YEARFRAC`

### Filter

`ALL`, `ALLCROSSFILTERED`, `ALLEXCEPT`, `ALLNOBLANKROW`, `ALLSELECTED`, `ALLSELECTEDAPPLY`, `ALLSELECTEDREMOVE`, `ALWAYSAPPLY`, `CALCULATE`, `CALCULATETABLE`, `DEPENDON`, `FILTER`, `FILTERCLUSTER`, `GROUPCROSSAPPLY`, `GROUPCROSSAPPLYTABLE`, `INDEX`, `KEEPFILTERS`, `LOOKUP`, `LOOKUPVALUE`, `LOOKUPWITHTOTALS`, `MATCHBY`, `NONFILTER`, `OFFSET`, `ORDERBY`, `PARTITIONBY`, `REMOVEFILTERS`, `SELECTEDVALUE`, `SHADOWCLUSTER`, `WINDOW`

### Financial

`ACCRINT`, `ACCRINTM`, `AMORDEGRC`, `AMORLINC`, `COUPDAYBS`, `COUPDAYS`, `COUPDAYSNC`, `COUPNCD`, `COUPNUM`, `COUPPCD`, `CUMIPMT`, `CUMPRINC`, `DB`, `DDB`, `DISC`, `DOLLARDE`, `DOLLARFR`, `DURATION`, `EFFECT`, `FV`, `INTRATE`, `IPMT`, `ISPMT`, `MDURATION`, `NOMINAL`, `NPER`, `ODDFPRICE`, `ODDFYIELD`, `ODDLPRICE`, `ODDLYIELD`, `PDURATION`, `PMT`, `PPMT`, `PRICE`, `PRICEDISC`, `PRICEMAT`, `PV`, `RATE`, `RECEIVED`, `RRI`, `SLN`, `SYD`, `TBILLEQ`, `TBILLPRICE`, `TBILLYIELD`, `VDB`, `XIRR`, `XNPV`, `YIELD`, `YIELDDISC`, `YIELDMAT`

### Information

`COLUMNSTATISTICS`, `CONTAINS`, `CONTAINSROW`, `CONTAINSSTRING`, `CONTAINSSTRINGEXACT`, `CUSTOMDATA`, `EVALUATEANDLOG`, `HASONEFILTER`, `HASONEVALUE`, `INFO.VIEW.COLUMNS`, `INFO.VIEW.MEASURES`, `INFO.VIEW.RELATIONSHIPS`, `INFO.VIEW.TABLES`, `ISAFTER`, `ISBLANK`, `ISBOOLEAN`, `ISCROSSFILTERED`, `ISDATETIME`, `ISDECIMAL`, `ISDOUBLE`, `ISEMPTY`, `ISERROR`, `ISEVEN`, `ISFILTERED`, `ISINSCOPE`, `ISINT64`, `ISLOGICAL`, `ISNONTEXT`, `ISNUMBER`, `ISNUMERIC`, `ISODD`, `ISONORAFTER`, `ISSELECTEDMEASURE`, `ISSTRING`, `ISSUBTOTAL`, `ISTEXT`, `NAMEOF`, `SELECTEDMEASURE`, `SELECTEDMEASUREFORMATSTRING`, `SELECTEDMEASURENAME`, `TABLEOF`, `TEXTCONTAINS`, `TEXTSIMILARITY`, `USERCULTURE`, `USERNAME`, `USEROBJECTID`, `USERPRINCIPALNAME`, `INFO.ALTERNATEOFDEFINITIONS`, `INFO.ANNOTATIONS`, `INFO.ATTRIBUTEHIERARCHIES`, `INFO.ATTRIBUTEHIERARCHYSTORAGES`, `INFO.CALCDEPENDENCY`, `INFO.CALCULATIONGROUPS`, `INFO.CALCULATIONITEMS`, `INFO.CALENDARCOLUMNGROUPS`, `INFO.CALENDARCOLUMNREFERENCES`, `INFO.CALENDARS`, `INFO.CATALOGS`, `INFO.CHANGEDPROPERTIES`, `INFO.COLUMNPARTITIONSTORAGES`, `INFO.COLUMNPERMISSIONS`, `INFO.COLUMNS`, `INFO.COLUMNSTORAGES`, `INFO.CSDLMETADATA`, `INFO.CULTURES`, `INFO.DATACOVERAGEDEFINITIONS`, `INFO.DATASOURCES`, `INFO.DELTATABLEMETADATASTORAGES`, `INFO.DEPENDENCIES`, `INFO.DETAILROWSDEFINITIONS`, `INFO.DICTIONARYSTORAGES`, `INFO.EXCLUDEDARTIFACTS`, `INFO.EXPRESSIONS`, `INFO.EXTENDEDPROPERTIES`, `INFO.FORMATSTRINGDEFINITIONS`, `INFO.FUNCTIONS`, `INFO.GENERALSEGMENTMAPSEGMENTMETADATASTORAGES`, `INFO.GROUPBYCOLUMNS`, `INFO.HIERARCHIES`, `INFO.HIERARCHYSTORAGES`, `INFO.KPIS`, `INFO.LEVELS`, `INFO.LINGUISTICMETADATA`, `INFO.MEASURES`, `INFO.MODEL`, `INFO.OBJECTTRANSLATIONS`, `INFO.PARQUETFILESTORAGES`, `INFO.PARTITIONS`, `INFO.PARTITIONSTORAGES`, `INFO.PERSPECTIVECOLUMNS`, `INFO.PERSPECTIVEHIERARCHIES`, `INFO.PERSPECTIVEMEASURES`, `INFO.PERSPECTIVES`, `INFO.PERSPECTIVETABLES`, `INFO.PROPERTIES`, `INFO.QUERYGROUPS`, `INFO.REFRESHPOLICIES`, `INFO.RELATEDCOLUMNDETAILS`, `INFO.RELATIONSHIPINDEXSTORAGES`, `INFO.RELATIONSHIPS`, `INFO.RELATIONSHIPSTORAGES`, `INFO.ROLEMEMBERSHIPS`, `INFO.ROLES`, `INFO.SEGMENTMAPSTORAGES`, `INFO.SEGMENTSTORAGES`, `INFO.STORAGEFILES`, `INFO.STORAGEFOLDERS`, `INFO.STORAGETABLECOLUMNS`, `INFO.STORAGETABLECOLUMNSEGMENTS`, `INFO.TABLEPERMISSIONS`, `INFO.TABLES`, `INFO.TABLESTORAGES`, `INFO.USERDEFINEDFUNCTIONS`, `INFO.VARIATIONS`

### Logical

`AND`, `BITAND`, `BITOR`, `BITXOR`, `COALESCE`, `FALSE`, `IF`, `IF.EAGER`, `IFERROR`, `NOT`, `OR`, `SWITCH`, `TRUE`

### Math and trig

`ABS`, `ACOS`, `ACOSH`, `ACOT`, `ACOTH`, `ASIN`, `ASINH`, `ATAN`, `ATANH`, `CEILING`, `CONVERT`, `COS`, `COSH`, `COT`, `COTH`, `CURRENCY`, `DEGREES`, `DIVIDE`, `EVEN`, `EXP`, `FACT`, `FLOOR`, `GCD`, `INT`, `ISO.CEILING`, `LCM`, `LN`, `LOG`, `LOG10`, `MOD`, `MROUND`, `ODD`, `PI`, `POWER`, `QUOTIENT`, `RADIANS`, `RAND`, `RANDBETWEEN`, `ROUND`, `ROUNDDOWN`, `ROUNDUP`, `SIGN`, `SIN`, `SINH`, `SQRT`, `SQRTPI`, `TAN`, `TANH`, `TRUNC`

### Other

`BITLSHIFT`, `BITRSHIFT`, `BLANK`, `EARLIER`, `EARLIEST`, `ERROR`, `EXTERNALMEASURE`, `HASH`, `INFO.STORAGETABLES`, `ISATLEVEL`, `ISCURRENCY`, `ISINTEGER`, `KEYWORDMATCH`, `RANK`, `ROWNUMBER`, `SAMPLECARTESIANPOINTSBYCOVER`

### Parent and child

`PATH`, `PATHCONTAINS`, `PATHITEM`, `PATHITEMREVERSE`, `PATHLENGTH`

### Relationships

`CROSSFILTER`, `RELATED`, `RELATEDTABLE`, `USERELATIONSHIP`

### Statistical

`BETA.DIST`, `BETA.INV`, `CHISQ.DIST`, `CHISQ.DIST.RT`, `CHISQ.INV`, `CHISQ.INV.RT`, `COMBIN`, `COMBINA`, `CONFIDENCE.NORM`, `CONFIDENCE.T`, `EXPON.DIST`, `GEOMEAN`, `GEOMEANX`, `LINEST`, `LINESTX`, `MEDIAN`, `MEDIANX`, `NORM.DIST`, `NORM.INV`, `NORM.S.DIST`, `NORM.S.INV`, `PERCENTILE.EXC`, `PERCENTILE.INC`, `PERCENTILEX.EXC`, `PERCENTILEX.INC`, `PERMUT`, `POISSON.DIST`, `RANK.EQ`, `RANKX`, `SAMPLE`, `STDEV.P`, `STDEV.S`, `STDEVX.P`, `STDEVX.S`, `T.DIST`, `T.DIST.2T`, `T.DIST.RT`, `T.INV`, `T.INV.2T`, `VAR.P`, `VAR.S`, `VARX.P`, `VARX.S`

### Table manipulation

`ADDCOLUMNS`, `ADDMISSINGITEMS`, `CROSSJOIN`, `CURRENTGROUP`, `DATATABLE`, `DETAILROWS`, `DISTINCT`, `EXCEPT`, `FILTERS`, `GENERATE`, `GENERATEALL`, `GENERATESERIES`, `GROUPBY`, `IGNORE`, `INTERSECT`, `NATURALINNERJOIN`, `NATURALJOINUSAGE`, `NATURALLEFTOUTERJOIN`, `NONVISUAL`, `ROLLUP`, `ROLLUPADDISSUBTOTAL`, `ROLLUPGROUP`, `ROLLUPISSUBTOTAL`, `ROW`, `SAMPLEAXISWITHLOCALMINMAX`, `SELECTCOLUMNS`, `SUBSTITUTEWITHINDEX`, `SUMMARIZE`, `SUMMARIZECOLUMNS`, `TOPN`, `TOPNPERLEVEL`, `TOPNSKIP`, `TREATAS`, `UNION`, `VALUES`

### Text

`COMBINEVALUES`, `CONCATENATE`, `CONCATENATEX`, `EXACT`, `FIND`, `FIXED`, `FORMAT`, `LEFT`, `LEN`, `LOWER`, `MID`, `REPLACE`, `REPT`, `RIGHT`, `SEARCH`, `SUBSTITUTE`, `TOCSV`, `TOJSON`, `TRIM`, `UNICHAR`, `UNICODE`, `UPPER`, `VALUE`

### Time intelligence

`CLOSINGBALANCEMONTH`, `CLOSINGBALANCEQUARTER`, `CLOSINGBALANCEWEEK`, `CLOSINGBALANCEYEAR`, `DATEADD`, `DATESBETWEEN`, `DATESINPERIOD`, `DATESMTD`, `DATESQTD`, `DATESWTD`, `DATESYTD`, `ENDOFMONTH`, `ENDOFQUARTER`, `ENDOFWEEK`, `ENDOFYEAR`, `FIRSTDATE`, `FIRSTNONBLANK`, `FIRSTNONBLANKVALUE`, `LASTDATE`, `LASTNONBLANK`, `LASTNONBLANKVALUE`, `NEXTDAY`, `NEXTMONTH`, `NEXTQUARTER`, `NEXTWEEK`, `NEXTYEAR`, `OPENINGBALANCEMONTH`, `OPENINGBALANCEQUARTER`, `OPENINGBALANCEWEEK`, `OPENINGBALANCEYEAR`, `PARALLELPERIOD`, `PREVIOUSDAY`, `PREVIOUSMONTH`, `PREVIOUSQUARTER`, `PREVIOUSWEEK`, `PREVIOUSYEAR`, `SAMEPERIODLASTYEAR`, `STARTOFMONTH`, `STARTOFQUARTER`, `STARTOFWEEK`, `STARTOFYEAR`, `TOTALMTD`, `TOTALQTD`, `TOTALWTD`, `TOTALYTD`

### Visual calculations

`COLLAPSE`, `COLLAPSEALL`, `EXPAND`, `EXPANDALL`, `FIRST`, `LAST`, `MOVINGAVERAGE`, `NEXT`, `PREVIOUS`, `RANGE`, `RUNNINGSUM`

## Notes

- `Evaluated` means a dispatch path exists: either the built-in switch in
  `crates/dax/src/eval.rs` (`Evaluator::eval`/`call_table`) or a family module under
  `crates/dax/src/eval/`; table-valued functions are counted when they have a `call_table` path.
  In this crate only the built-in switch is linked (`FAMILIES` is empty), which is the subset the
  scope paragraph above lists.
- Engine-only functions (internal filter clusters, `INFO.*` storage views, visual calculations)
  have no evaluator in this crate: like every other catalogued name outside the scalar core they
  report `not implemented` until the family that owns them is linked. Where a family is linked it
  documents any bounded equivalent in its own module; `MS Learn` and dax.guide are the semantic
  references throughout.
- The catalog is the dax.guide aggregation/data-type/statement/operator surface plus every
  function category in `crates/dax/src/functions/`; see `docs/parity-checklist.md` for the
  GridCraft command surface.
