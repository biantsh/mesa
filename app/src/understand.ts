import type { Category } from "./api";
import { cleanQuery } from "./cleanQuery";

export interface Period {
  label: string;
  month: Month;
  start: number;
  end: number;
}

export interface Understood {
  fileType: Category | null;
  from: Period | null;
  to: Period | null;
  text: string;
  fileTypeWords: string[];
  dateWords: string[];
}

const NOTHING: Understood = { fileType: null, from: null, to: null, text: "", fileTypeWords: [], dateWords: [] };

interface Token {
  word: string;
  start: number;
  end: number;
}

interface Span {
  start: number;
  end: number;
}

export type Month = number;

export const monthOf = (year: number, month: number): Month => year * 12 + month;
const dateToMonth = (date: Date): Month => monthOf(date.getFullYear(), date.getMonth());

export const MONTH_LABELS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

function period(month: Month): Period {
  const year = Math.floor(month / 12);
  const number = month - year * 12;
  return {
    label: `${MONTH_LABELS[number]} ${year}`,
    month,
    start: new Date(year, number, 1).getTime(),
    end: new Date(year, number + 1, 1).getTime(),
  };
}

function tokenize(query: string): Token[] {
  const tokens: Token[] = [];
  const chunk = /[^\s,;]+/g;
  for (let match = chunk.exec(query); match; match = chunk.exec(query)) {
    const raw = match[0];
    const lead = raw.search(/[^()[\]{}"“”:!?.*]/);
    if (lead < 0) continue;
    const trail = raw.slice(lead).replace(/[()[\]{}"“”:!?.]+$/, "").length;
    const start = match.index + lead;
    const word = normalize(raw.slice(lead, lead + trail));
    const prefix = /^(mid|pre|post|early|late)-(.+)$/.exec(word);
    const months = /^([a-z]+)-([a-z]+)$/.exec(word);
    const end = start + trail;
    if (prefix) {
      const first = prefix[1] + (prefix[1] === "pre" || prefix[1] === "post" ? "-" : "");
      tokens.push({ word: first, start, end: start + prefix[1].length + 1 }, { word: prefix[2], start: start + prefix[1].length + 1, end });
    } else if (months && MONTHS.has(months[1]) && MONTHS.has(months[2])) {
      const dash = start + months[1].length;
      tokens.push({ word: months[1], start, end: dash }, { word: "-", start: dash, end: dash + 1 }, { word: months[2], start: dash + 1, end });
    } else {
      tokens.push({ word, start, end });
    }
  }
  return tokens;
}

function normalize(word: string): string {
  return word
    .toLowerCase()
    .replace(/’/g, "'")
    .replace(/[–—]/g, "-")
    .replace(/^(?!where|what|who|it|that)([a-z]+)'s$/, "$1");
}

const words = (list: string) => new Set(list.split(" "));

const MONTHS = new Map<string, number>(
  (
    "january jan|february feb|march mar|april apr|may|june jun|july jul|august aug|" +
    "september sep sept|october oct|november nov|december dec"
  )
    .split("|")
    .flatMap((names, number) => names.split(" ").map((name) => [name, number] as [string, number])),
);

const WORD_MONTHS = words("april may june august march");
const PEOPLE_MONTHS = words("april may june august jan");
const SENT_THINGS = words("letter letters email emails mail message messages text texts gift gifts card cards call calls");

const SEASONS = new Map<string, number>([
  ["spring", 2],
  ["summer", 5],
  ["fall", 8],
  ["autumn", 8],
  ["winter", 11],
]);

const NUMBER_WORDS = new Map<string, number>([
  ["a", 1],
  ["an", 1],
  ["one", 1],
  ["two", 2],
  ["three", 3],
  ["four", 4],
  ["five", 5],
  ["six", 6],
  ["seven", 7],
  ["eight", 8],
  ["nine", 9],
  ["ten", 10],
  ["eleven", 11],
  ["twelve", 12],
]);

const DATE_WORDS_BEFORE = words("in from during dated circa around taken shot on of roughly approximately");

const COUNTED = words(
  "people persons men women kids students dollars bucks usd eur euros pounds gbp yen miles km kilometers meters metres feet ft " +
    "words pages lines items rows calories kcal pixels px mg ml kg lbs points steps horsepower hp times units tons gb mb kb rpm volts",
);
const CARS = words("toyota honda ford bmw audi mercedes tesla chevy chevrolet nissan vw volkswagen hyundai kia subaru mazda jeep car truck");
const NAMED_BY_NUMBER = words(
  "iphone ipad windows win office excel word powerpoint outlook ubuntu galaxy pixel surface model version ver v route highway room page pages " +
    "chapter section part area apollo top over under about number no size code id item order fifa nba nfl madden class " +
    "tax taxes budget fiscal fy w2 w-2 form return returns annual report statement invoice",
);

const LEFT_OF_DATE = words(
  "from in of on during within around circa dated taken shot made created modified changed edited saved downloaded uploaded captured recorded filmed written got took " +
    "back way sometime roughly approximately last this earlier later early mid late",
);
const LEFT_OF_TYPE = words("a an the my our this that these those some any every each in as on into");
const RIGHT_OF_TYPE = words("of showing containing featuring called named titled file files");
const OPENERS = [
  "can you find me",
  "can you find",
  "can you show me",
  "can you show",
  "i'm looking for",
  "im looking for",
  "i am looking for",
  "looking for",
  "look for",
  "search for",
  "searching for",
  "where is",
  "where are",
  "where's",
  "wheres",
  "i want",
  "i need",
  "find me",
  "show me",
  "get me",
  "give me",
  "find",
  "show",
  "please",
].map((phrase) => phrase.split(" "));
const LEADING = words("a an the my our some any all every each of with from about showing containing featuring that which and or please file files stuff something anything thing things");
const TRAILING = words("a an the my our of with from in on at and or by to for that which");

const hasContent = (word: string) => /[\p{L}\p{N}]/u.test(word);

interface Range {
  from: Month | null;
  to: Month | null;
}

interface Atom {
  next: number;
  lo: Month;
  hi: Month;
  kind: "year" | "open" | "sure";
  month?: number;
}

const AGE_MONTHS = { day: 0, week: 0, month: 1, year: 12, yr: 12, decade: 120 };

class DateReader {
  readonly thisYear: number;
  readonly thisMonth: Month;

  constructor(
    readonly tokens: Token[],
    readonly now: Date,
  ) {
    this.thisYear = now.getFullYear();
    this.thisMonth = dateToMonth(now);
  }

  word(at: number): string {
    return at >= 0 && at < this.tokens.length ? this.tokens[at].word : "";
  }

  daysBack(days: number): Month {
    return dateToMonth(new Date(this.now.getFullYear(), this.now.getMonth(), this.now.getDate() - days));
  }

  says(at: number, phrase: string): number {
    const parts = phrase.split(" ");
    return parts.every((part, offset) => this.word(at + offset) === part) ? at + parts.length : -1;
  }

  saysAny(at: number, phrases: string[]): number {
    for (const phrase of phrases) {
      const next = this.says(at, phrase);
      if (next >= 0) return next;
    }
    return -1;
  }

  yearAt(at: number, twoDigits = false): number | null {
    const word = this.word(at);
    const whole = /^\d{4}$/.test(word) ? Number(word) : null;
    if (whole !== null) return whole >= 1970 && whole <= this.thisYear ? whole : null;
    const short = /^'(\d{2})$/.exec(word) ?? (twoDigits ? /^(\d{2})$/.exec(word) : null);
    if (!short) return null;
    const year = 2000 + Number(short[1]);
    return year <= this.thisYear ? year : year - 100;
  }

  recentMonth(month: number): Month {
    const here = monthOf(this.thisYear, month);
    return here <= this.thisMonth ? here : here - 12;
  }

  dayAt(at: number): number | null {
    const day = /^(\d{1,2})(st|nd|rd|th)?$/.exec(this.word(at));
    return day && Number(day[1]) >= 1 && Number(day[1]) <= 31 ? Number(day[1]) : null;
  }

  counts(at: number): boolean {
    return COUNTED.has(this.word(at));
  }

  wholeYear(at: number): { next: number; year: number } | null {
    const skipped = this.says(at, "the year");
    const start = skipped >= 0 ? skipped : this.says(at, "year") >= 0 ? at + 1 : at;
    const year = this.yearAt(start);
    if (year !== null) return { next: start + 1, year };
    const last = this.says(at, "last year");
    if (last >= 0) return { next: last, year: this.thisYear - 1 };
    const current = this.says(at, "this year");
    if (current >= 0) return { next: current, year: this.thisYear };
    return null;
  }

  partOfYear(at: number): Atom | null {
    const start = this.says(at, "the") >= 0 ? at + 1 : at;
    const parts: [string, number, number, number?][] = [
      ["early", 0, 3, 0],
      ["mid", 4, 7, 3],
      ["middle of", 4, 7],
      ["late", 8, 11, 7],
      ["beginning of", 0, 2],
      ["start of", 0, 2],
      ["end of", 9, 11],
      ["first half of", 0, 5],
      ["second half of", 6, 11],
      ["q1", 0, 2],
      ["q2", 3, 5],
      ["q3", 6, 8],
      ["q4", 9, 11],
      ["first quarter of", 0, 2],
      ["second quarter of", 3, 5],
      ["third quarter of", 6, 8],
      ["fourth quarter of", 9, 11],
    ];
    for (const [phrase, first, last, intoDecade] of parts) {
      const next = this.says(start, phrase);
      if (next < 0) continue;
      const year = this.wholeYear(next);
      if (year && !this.counts(year.next)) {
        return { next: year.next, lo: monthOf(year.year, first), hi: monthOf(year.year, last), kind: "sure" };
      }
      const decade = intoDecade !== undefined ? this.decadeAt(next) : null;
      if (decade) {
        const lo = decade.lo + intoDecade! * 12;
        return { next: decade.next, lo, hi: lo + 35, kind: "open" };
      }
      if (MONTHS.has(this.word(next))) return this.monthPhrase(next);
    }
    return null;
  }

  decadeAt(at: number): Atom | null {
    const start = this.says(at, "the") >= 0 ? at + 1 : at;
    const word = this.word(start);
    const named = ["eighties", "nineties"].indexOf(word);
    if (named >= 0) return this.decade(start + 1, 1980 + named * 10);
    const match = /^'?(\d{2}|\d{4})'?s$/.exec(word);
    if (!match) return null;
    const number = Number(match[1]);
    if (number % 10 !== 0) return null;
    const year = match[1].length === 4 ? number : number + (number >= 70 ? 1900 : 2000);
    return year >= 1970 && year <= this.thisYear ? this.decade(start + 1, year) : null;
  }

  decade(next: number, year: number): Atom {
    return { next, lo: monthOf(year, 0), hi: monthOf(year + 9, 11), kind: "open" };
  }

  isNamedDay(month: number, day: number): boolean {
    return (month === 6 && day === 4) || (month === 8 && day === 11);
  }

  monthPhrase(at: number): Atom | null {
    const start = this.says(at, "the") >= 0 ? at + 1 : at;
    const day = this.dayAt(start);
    if (day !== null) {
      const name = this.says(start + 1, "of") >= 0 ? start + 2 : start + 1;
      const month = MONTHS.get(this.word(name));
      if (month === undefined) return null;
      const year = this.yearAt(name + 1, true);
      if (year !== null) return { next: name + 2, lo: monthOf(year, month), hi: monthOf(year, month), kind: "sure" };
      if (/^\d{4}$/.test(this.word(name + 1)) || this.isNamedDay(month, day)) return null;
      const recent = this.recentMonth(month);
      return { next: name + 1, lo: recent, hi: recent, kind: "sure", month };
    }
    const before = this.yearAt(start);
    const after = before !== null ? MONTHS.get(this.word(start + 1)) : undefined;
    if (before !== null && after !== undefined) {
      return { next: start + 2, lo: monthOf(before, after), hi: monthOf(before, after), kind: "sure" };
    }

    const month = MONTHS.get(this.word(start));
    if (month === undefined) return null;
    const of = this.says(start + 1, "of") >= 0 ? start + 2 : start + 1;
    const year = this.yearAt(of, of === start + 1 ? false : true);
    if (year !== null && !this.counts(of + 1)) return { next: of + 1, lo: monthOf(year, month), hi: monthOf(year, month), kind: "sure" };
    if (/^\d{4}$/.test(this.word(start + 1))) return null;
    const dayAfter = this.dayAt(start + 1);
    if (dayAfter !== null && !this.counts(start + 2)) {
      const withYear = this.yearAt(start + 2);
      if (withYear !== null) return { next: start + 3, lo: monthOf(withYear, month), hi: monthOf(withYear, month), kind: "sure" };
      if (/^\d{4}$/.test(this.word(start + 2)) || this.isNamedDay(month, dayAfter)) return null;
      const recent = this.recentMonth(month);
      return { next: start + 2, lo: recent, hi: recent, kind: "sure", month };
    }
    const recent = this.recentMonth(month);
    return { next: start + 1, lo: recent, hi: recent, kind: "open", month };
  }

  seasonPhrase(at: number): Atom | null {
    const start = this.says(at, "the") >= 0 ? at + 1 : at;
    const season = SEASONS.get(this.word(start));
    if (season !== undefined) {
      const of = this.says(start + 1, "of") >= 0 ? start + 2 : start + 1;
      const year = this.yearAt(of, true);
      if (year === null || this.counts(of + 1)) return null;
      const lo = season === 11 ? monthOf(year, 0) : monthOf(year, season);
      const hi = season === 11 ? monthOf(year + 1, 1) : monthOf(year, season + 2);
      return { next: of + 1, lo, hi, kind: "sure" };
    }
    return null;
  }

  relative(at: number): Atom | null {
    const sure = (next: number, lo: Month, hi: Month): Atom => ({ next, lo, hi, kind: "sure" });
    const word = this.word(at);
    const after = this.word(at + 1);

    if (word === "today" || word === "todays" || word === "tonight") return sure(at + 1, this.thisMonth, this.thisMonth);
    if (word === "yesterday" || word === "yesterdays") return sure(at + 1, this.daysBack(1), this.daysBack(1));
    if (word === "recently" || word === "lately" || word === "recent") return sure(at + 1, this.thisMonth - 6, this.thisMonth);
    if (word === "most" && after === "recent") return sure(at + 2, this.thisMonth - 6, this.thisMonth);
    if (word === "this" && /^(morning|afternoon|evening)$/.test(after)) return sure(at + 2, this.thisMonth, this.thisMonth);
    if (word === "last" && after === "night") return sure(at + 2, this.daysBack(1), this.daysBack(1));

    if (word === "half" && this.saysAny(at + 1, ["a year ago", "a year back"]) >= 0) return sure(at + 4, this.thisMonth - 6, this.thisMonth - 6);

    if (word === "this" || word === "last" || word === "previous" || word === "past") {
      const past = word !== "this";
      if (after === "week" || after === "weekend") {
        return sure(at + 2, this.daysBack(past ? 14 : 7), this.thisMonth);
      }
      if (after === "month") {
        const month = past ? this.thisMonth - 1 : this.thisMonth;
        return sure(at + 2, month, month);
      }
      if (after === "year" || after === "years") {
        const year = past ? this.thisYear - 1 : this.thisYear;
        return sure(at + 2, monthOf(year, 0), monthOf(year, 11));
      }
      if (after === "quarter") {
        const lo = this.thisMonth - this.thisMonth % 3 - (past ? 3 : 0);
        return sure(at + 2, lo, lo + 2);
      }
      const season = SEASONS.get(after);
      if (season !== undefined) {
        for (let year = this.thisYear; ; year--) {
          const lo = monthOf(year, season);
          const hi = lo + 2;
          if (past ? hi < this.thisMonth : lo <= this.thisMonth) return sure(at + 2, lo, hi);
        }
      }
      const month = MONTHS.get(after);
      if (month !== undefined && word !== "previous") {
        const recent = this.recentMonth(month);
        const lo = past && recent === this.thisMonth ? recent - 12 : recent;
        return sure(at + 2, lo, lo);
      }
    }

    if (word === "the" && this.says(at + 1, "year before last") >= 0) {
      return sure(at + 4, monthOf(this.thisYear - 2, 0), monthOf(this.thisYear - 2, 11));
    }

    return this.ago(at);
  }

  amount(at: number): { next: number; near: number; far: number } | null {
    const word = this.word(at);
    if (/^\d{1,3}$/.test(word)) return { next: at + 1, near: Number(word), far: Number(word) };
    const number = NUMBER_WORDS.get(word);
    if (number !== undefined) {
      if ((word === "a" || word === "an") && this.says(at + 1, "couple") >= 0) return this.amount(at + 1);
      if ((word === "a" || word === "an") && this.says(at + 1, "few") >= 0) return this.amount(at + 1);
      return { next: at + 1, near: number, far: number };
    }
    const fuzzy = word === "couple" ? [1, 3] : word === "few" ? [2, 5] : word === "several" ? [3, 7] : null;
    if (!fuzzy) return null;
    const next = this.says(at + 1, "of") >= 0 ? at + 2 : at + 1;
    return { next, near: fuzzy[0], far: fuzzy[1] };
  }

  back(unit: string, near: number, far: number): { lo: Month; hi: Month } | null {
    const singular = unit.replace(/s$/, "");
    if (singular === "day") return { lo: this.daysBack(far), hi: this.daysBack(near) };
    if (singular === "week") return { lo: this.daysBack((far + 1) * 7), hi: this.daysBack(Math.max(0, near - 1) * 7) };
    if (singular === "month") return { lo: this.thisMonth - far, hi: this.thisMonth - near };
    if (singular === "year" || singular === "yr") return { lo: monthOf(this.thisYear - far, 0), hi: monthOf(this.thisYear - near, 11) };
    if (singular === "decade") return { lo: monthOf(this.thisYear - far * 10, 0), hi: monthOf(this.thisYear - near * 10, 11) };
    return null;
  }

  ago(at: number): Atom | null {
    const start = this.saysAny(at, ["about", "around", "roughly", "almost", "nearly"]);
    const amount = this.amount(start >= 0 ? start : at);
    if (!amount) return null;
    const back = this.back(this.word(amount.next), amount.near, amount.far);
    if (!back || !/^(ago|back)$/.test(this.word(amount.next + 1))) return null;
    return { next: amount.next + 2, lo: back.lo, hi: back.hi, kind: "sure" };
  }

  digits(at: number): Atom | null {
    const word = this.word(at);
    const known = (year: number) => year >= 1970 && year <= this.thisYear;
    const sure = (lo: Month, hi: Month): Atom => ({ next: at + 1, lo: Math.min(lo, hi), hi: Math.max(lo, hi), kind: "sure" });
    const inRange = (month: number) => month >= 1 && month <= 12;

    let match = /^(\d{4})[-/.](\d{1,2})[-/.](\d{1,2})$/.exec(word);
    if (match) {
      if (!known(Number(match[1])) || !inRange(Number(match[2]))) return null;
      return sure(monthOf(Number(match[1]), Number(match[2]) - 1), monthOf(Number(match[1]), Number(match[2]) - 1));
    }
    match = /^(\d{1,2})[-/.](\d{1,2})[-/.](\d{4}|\d{2})$/.exec(word);
    if (match) {
      const year = match[3].length === 4 ? Number(match[3]) : 2000 + Number(match[3]);
      if (!known(year)) return null;
      const [a, b] = [Number(match[1]), Number(match[2])];
      if (a > 12 && b > 12) return null;
      if (a > 12) return sure(monthOf(year, b - 1), monthOf(year, b - 1));
      if (b > 12) return sure(monthOf(year, a - 1), monthOf(year, a - 1));
      if (a === 0 || b === 0) return null;
      return sure(monthOf(year, a - 1), monthOf(year, b - 1));
    }
    match = /^(\d{4})[-/.](\d{2})$/.exec(word);
    if (match) {
      const [year, second] = [Number(match[1]), Number(match[2])];
      if (!known(year)) return null;
      if (inRange(second)) return sure(monthOf(year, second - 1), monthOf(year, second - 1));
      const end = Math.floor(year / 100) * 100 + second;
      return end > year && known(end) ? sure(monthOf(year, 0), monthOf(end, 11)) : null;
    }
    match = /^(\d{4})-(\d{4})$/.exec(word);
    if (match) {
      if (!known(Number(match[1])) || !known(Number(match[2]))) return null;
      return sure(monthOf(Number(match[1]), 0), monthOf(Number(match[2]), 11));
    }
    match = /^(\d{1,2})\/(\d{4})$/.exec(word) ?? /^(\d{2})-(\d{4})$/.exec(word);
    if (match) {
      if (!inRange(Number(match[1])) || !known(Number(match[2]))) return null;
      return sure(monthOf(Number(match[2]), Number(match[1]) - 1), monthOf(Number(match[2]), Number(match[1]) - 1));
    }
    return null;
  }

  atom(at: number): Atom | null {
    const digits = this.digits(at);
    if (digits) return digits;
    const part = this.partOfYear(at);
    if (part) return part;
    const season = this.seasonPhrase(at);
    if (season) return season;
    const relative = this.relative(at);
    if (relative) return relative;
    const month = this.monthPhrase(at);
    if (month) return month;
    const decade = this.decadeAt(at);
    if (decade) return decade;
    const year = this.wholeYear(at);
    if (year) {
      if (this.counts(year.next)) return null;
      return { next: year.next, lo: monthOf(year.year, 0), hi: monthOf(year.year, 11), kind: "year" };
    }
    return null;
  }

  bareYearIsDate(at: number, atom: Atom): boolean {
    const before = at > 0 ? this.word(at - 1) : "";
    if (NAMED_BY_NUMBER.has(before)) return false;
    if (before === "of" && this.word(at - 2) === "class") return false;
    const after = this.word(atom.next);
    return after !== "by" && !CARS.has(after) && !CARS.has(before);
  }

  joined(first: Atom, last: Atom): { lo: Month; hi: Month } {
    if (first.month !== undefined) {
      const hi = last.month !== undefined ? last.lo : last.hi;
      const lo = monthOf(Math.floor(hi / 12), first.month);
      return { lo: lo > hi ? lo - 12 : lo, hi };
    }
    if (last.month !== undefined) {
      const hi = monthOf(Math.floor(first.hi / 12), last.month);
      return { lo: first.lo, hi: hi < first.lo ? hi + 12 : hi };
    }
    return { lo: Math.min(first.lo, last.lo), hi: Math.max(first.hi, last.hi) };
  }

  after(at: number): { next: number; open: "from" | "to" } | null {
    const later = this.saysAny(at, ["or later", "or after", "or newer", "and later", "and after", "and newer", "and up", "onwards", "onward"]);
    if (later >= 0) return { next: later, open: "to" };
    const earlier = this.saysAny(at, ["or earlier", "or before", "or older", "and earlier", "and before", "and older"]);
    if (earlier >= 0) return { next: earlier, open: "from" };
    return null;
  }

  read(at: number): (Span & Range) | null {
    return this.between(at) ?? this.comparison(at) ?? this.window(at) ?? this.around(at) ?? this.afterWord(at) ?? this.onItsOwn(at);
  }

  between(at: number): (Span & Range) | null {
    const word = this.word(at);
    const led = word === "between" || (DATE_WORDS_BEFORE.has(word) && word !== "of" && word !== "on");
    const start = led ? at + 1 : at;
    const first = this.atom(start);
    if (!first) return null;
    const links = word === "between" ? ["and", "&", "to", "-"] : ["to", "until", "till", "through", "thru", "-"];
    const link = this.saysAny(first.next, links);
    if (link < 0) return null;
    const last = this.atom(link);
    if (!last) return null;
    if (!led && first.kind === "open" && last.kind === "open") return null;
    if (!led && first.kind === "year" && !this.bareYearIsDate(at, first)) return null;
    const { lo, hi } = this.joined(first, last);
    return { start: at, end: last.next, from: lo, to: hi };
  }

  comparison(at: number): (Span & Range) | null {
    const compare = (phrases: string[], make: (atom: Atom) => Range): (Span & Range) | null => {
      const next = this.saysAny(at, phrases);
      if (next < 0) return null;
      const date = this.atom(next);
      return date ? { start: at, end: date.next, ...make(date) } : null;
    };
    const before = (date: Atom): Range => ({ from: null, to: date.lo - 1 });
    const upTo = (date: Atom): Range => ({ from: null, to: date.hi });
    const later = (date: Atom): Range => ({ from: date.hi + 1, to: null });
    const since = (date: Atom): Range => ({ from: date.lo, to: null });

    if (this.word(at) === "after" && this.word(at - 1) === "and" && this.word(at - 2) === "before") return null;
    const age = this.age(at);
    return (
      age ??
      compare(["before", "pre-", "prior to", "earlier than", "older than"], before) ??
      compare(["until", "till", "through", "thru", "up to", "no later than", "not after"], upTo) ??
      compare(["after", "post-", "later than", "newer than", "more recent than"], later) ??
      compare(["since", "starting from", "starting in", "starting", "beginning in", "no earlier than", "not before"], since)
    );
  }

  age(at: number): (Span & Range) | null {
    for (const [phrase, older] of [
      ["older than", true],
      ["more than", true],
      ["over", true],
      ["newer than", false],
      ["less than", false],
      ["under", false],
    ] as [string, boolean][]) {
      const next = this.says(at, phrase);
      if (next < 0) continue;
      const amount = this.amount(next);
      if (!amount) continue;
      const unit = this.word(amount.next);
      const singular = unit.replace(/s$/, "");
      const months = (AGE_MONTHS as Record<string, number | undefined>)[singular];
      if (months === undefined) continue;
      const tail = this.word(amount.next + 1);
      const ended = tail === "ago" || tail === "old" || tail === "back";
      if (!ended && !phrase.endsWith("than")) continue;
      if (!ended && phrase === "more than") continue;
      if (!ended && phrase === "less than") continue;
      const end = amount.next + (ended ? 2 : 1);
      const edge = months === 0 ? this.daysBack(amount.far * (singular === "week" ? 7 : 1)) : this.thisMonth - amount.far * months;
      return older ? { start: at, end, from: null, to: edge } : { start: at, end, from: edge, to: null };
    }
    return null;
  }

  window(at: number): (Span & Range) | null {
    let start = at;
    if (this.saysAny(start, ["within", "in", "over", "from", "during"]) >= 0) start++;
    if (this.says(start, "the") >= 0) start++;
    if (this.saysAny(start, ["last", "past"]) < 0) return null;
    const after = this.word(start + 1);
    const amount = this.amount(start + 1);
    if (amount) {
      const unit = this.word(amount.next);
      const lo = this.windowStart(unit, amount.far);
      return lo !== null ? { start: at, end: amount.next + 1, from: lo, to: this.thisMonth } : null;
    }
    if (this.word(start) === "past") {
      const lo = this.windowStart(after, 1);
      if (lo !== null) return { start: at, end: start + 2, from: lo, to: this.thisMonth };
    }
    return null;
  }

  windowStart(unit: string, number: number): Month | null {
    const singular = unit.replace(/s$/, "");
    if (singular === "day") return this.daysBack(number);
    if (singular === "week") return this.daysBack(number * 7);
    if (singular === "month") return this.thisMonth - number;
    if (singular === "year" || singular === "yr") return this.thisMonth - number * 12;
    return null;
  }

  around(at: number): (Span & Range) | null {
    const next = this.saysAny(at, ["sometime around", "around", "circa", "roughly", "approximately"]);
    if (next < 0) return null;
    const date = this.atom(next);
    if (!date || date.kind === "open") return null;
    const by = date.kind === "year" ? 12 : 1;
    return { start: at, end: date.next, from: date.lo - by, to: date.hi + by };
  }

  afterWord(at: number): (Span & Range) | null {
    const led = DATE_WORDS_BEFORE.has(this.word(at));
    const opened = led ? at + 1 : at;
    const date = this.atom(opened);
    if (!date) return null;
    const after = this.after(date.next);
    if (this.word(at) === "of" && (date.kind === "open" || this.word(at - 1) === "class")) return null;
    const on = this.word(at) === "from" && this.says(date.next, "on") >= 0 ? date.next + 1 : -1;
    if (after === null && on < 0) return null;
    if (!led && date.kind === "open") return null;
    if (!led && date.kind === "year" && !this.bareYearIsDate(at, date)) return null;
    if (on >= 0) return { start: at, end: on, from: date.lo, to: null };
    return after!.open === "to"
      ? { start: at, end: after!.next, from: date.lo, to: null }
      : { start: at, end: after!.next, from: null, to: date.hi };
  }

  onItsOwn(at: number): (Span & Range) | null {
    const word = this.word(at);
    const led = DATE_WORDS_BEFORE.has(word);
    const start = led ? at + 1 : at;
    const date = this.atom(start);
    if (!date) return null;
    if (led) {
      const isMonth = date.month !== undefined || MONTHS.has(this.word(start));
      if (word === "of") {
        if (date.kind === "open" || isMonth || this.word(at - 1) === "class") return null;
      }
      if (word === "on" && date.kind !== "sure") return null;
      if (word === "from" && date.kind === "open" && PEOPLE_MONTHS.has(this.word(start)) && SENT_THINGS.has(this.word(at - 1))) return null;
      return this.withOthers(at, date);
    }
    if (date.kind === "open") return null;
    if (date.kind === "year" && (!this.bareYearIsDate(at, date) || /^(19\d\d|2000)$/.test(this.word(at)))) return null;
    return this.withOthers(at, date);
  }

  withOthers(at: number, date: Atom): Span & Range {
    let { lo, hi, next } = date;
    if (date.kind === "open" && hi - lo === 119) {
      for (;;) {
        const link = this.saysAny(next, ["and", "or", "&"]);
        const decade = link >= 0 ? this.decadeAt(link) : this.decadeAt(next);
        if (!decade) break;
        lo = Math.min(lo, decade.lo);
        hi = Math.max(hi, decade.hi);
        next = decade.next;
      }
    }
    if (date.kind === "year") {
      for (;;) {
        const link = this.saysAny(next, ["and", "or", "&"]);
        const year = link >= 0 ? this.yearAt(link) : this.yearAt(next);
        if (year === null || this.counts((link >= 0 ? link : next) + 1)) break;
        const here = link >= 0 ? link : next;
        lo = Math.min(lo, monthOf(year, 0));
        hi = Math.max(hi, monthOf(year, 11));
        next = here + 1;
      }
    }
    return { start: at, end: next, from: lo, to: hi };
  }
}

function readDates(tokens: Token[], now: Date, ignored: Set<string>, wordsOf: (span: Span) => string): { range: Range; spans: Span[] } {
  const reader = new DateReader(tokens, now);
  const range: Range = { from: null, to: null };
  const spans: Span[] = [];
  for (let at = 0; at < tokens.length; ) {
    const found = reader.read(at);
    if (!found) {
      at++;
      continue;
    }
    at = found.end;
    if (ignored.has(wordsOf(found))) continue;
    if (spans.length === 0) {
      range.from = found.from;
      range.to = found.to;
      spans.push(found);
      continue;
    }
    const haveBoth = range.from !== null && range.to !== null;
    const bothSides = found.from !== null && found.to !== null;
    if (haveBoth && bothSides) {
      range.from = Math.min(range.from!, found.from!);
      range.to = Math.max(range.to!, found.to!);
      spans.push(found);
    } else if (range.to === null && found.from === null && found.to! >= range.from!) {
      range.to = found.to;
      spans.push(found);
    } else if (range.from === null && found.to === null && found.from! <= range.to!) {
      range.from = found.from;
      spans.push(found);
    }
  }
  return { range, spans };
}

interface TypeWord {
  fileType: Category;
  keep: boolean;
}

function typeWords(fileType: Category, generic: string, descriptive = ""): [string, TypeWord][] {
  return [
    ...generic.split(" ").filter(Boolean).map((word): [string, TypeWord] => [word, { fileType, keep: false }]),
    ...descriptive.split(" ").filter(Boolean).map((word): [string, TypeWord] => [word, { fileType, keep: true }]),
  ];
}

const TYPE_WORDS = new Map<string, TypeWord>([
  ...typeWords("image", "photo photos photograph photographs pic pics picture pictures image images img imgs jpg jpgs jpeg jpegs jfif png pngs webp bmp tif tiff heic heif", "screenshot screenshots screencap screencaps selfie selfies meme memes wallpaper wallpapers gif gifs"),
  ...typeWords("video", "video videos vid vids mp4 mp4s m4v mov mkv avi webm", "movie movies footage vlog vlogs"),
  ...typeWords("audio", "audio mp3 mp3s wav flac m4a m4b aac aiff ogg", "song songs music podcast podcasts audiobook audiobooks"),
  ...typeWords("document", "document documents doc docs docx pdf pdfs txt rtf xlsx pptx", "resume resumes résumé résumés cv essay essays thesis dissertation ebook ebooks e-book e-books manuscript manuscripts spreadsheet spreadsheets"),
]);

const TYPE_PHRASES: [string, TypeWord][] = [
  ["screen recording", { fileType: "video", keep: true }],
  ["screen recordings", { fileType: "video", keep: true }],
  ["voice memo", { fileType: "audio", keep: true }],
  ["voice memos", { fileType: "audio", keep: true }],
  ["voice note", { fileType: "audio", keep: true }],
  ["voice notes", { fileType: "audio", keep: true }],
  ["voice message", { fileType: "audio", keep: true }],
  ["voice messages", { fileType: "audio", keep: true }],
  ["voice recording", { fileType: "audio", keep: true }],
  ["voice recordings", { fileType: "audio", keep: true }],
  ["sound file", { fileType: "audio", keep: false }],
  ["sound files", { fileType: "audio", keep: false }],
  ["sound clip", { fileType: "audio", keep: false }],
  ["sound clips", { fileType: "audio", keep: false }],
  ["audio book", { fileType: "audio", keep: true }],
  ["audio books", { fileType: "audio", keep: true }],
  ["video clip", { fileType: "video", keep: false }],
  ["video clips", { fileType: "video", keep: false }],
  ["music video", { fileType: "video", keep: true }],
  ["music videos", { fileType: "video", keep: true }],
  ["text file", { fileType: "document", keep: false }],
  ["text files", { fileType: "document", keep: false }],
  ["word doc", { fileType: "document", keep: false }],
  ["word docs", { fileType: "document", keep: false }],
  ["word document", { fileType: "document", keep: false }],
  ["word documents", { fileType: "document", keep: false }],
  ["word file", { fileType: "document", keep: false }],
  ["word files", { fileType: "document", keep: false }],
  ["cover letter", { fileType: "document", keep: true }],
  ["cover letters", { fileType: "document", keep: true }],
];

const NOT_A_TYPE = new Set(
  (
    "video game|video games|videogame|video call|video calls|video card|video cards|video camera|video cameras|video editor|video player|video store|" +
    "picture frame|picture frames|picture book|picture books|big picture|motion picture|" +
    "image processing|image editor|disk image|disk images|docker image|docker images|iso image|system image|" +
    "photo album|photo albums|photo booth|photo frame|photo frames|photo id|photo editor|photo printer|photo paper|photo shoot|" +
    "audio cable|audio cables|audio interface|audio jack|audio system|audio equipment|audio settings|audio driver|audio drivers|" +
    "music festival|music festivals|music teacher|music lesson|music lessons|music school|music store|music room|music stand|music theory|music class|sheet music|music sheet|" +
    "song lyrics|song list|" +
    "movie poster|movie posters|movie ticket|movie tickets|movie night|movie theater|movie theatre|movie review|movie reviews|movie script|movie list|movie still|movie stills|movie quote|movie quotes|" +
    "document camera|document scanner|documents folder|document folder|pdf editor|pdf reader|pdf viewer|cv joint|doc martens"
  ).split("|"),
);
const NOT = words("no not without non never");
const HEAD_OF = words("of showing containing featuring");

interface TypeCandidate extends Span, TypeWord {}

function typeCandidates(tokens: Token[], taken: boolean[], ignored: Set<string>, wordsOf: (span: Span) => string): TypeCandidate[] {
  const found: TypeCandidate[] = [];
  const free = (at: number) => at < tokens.length && !taken[at];
  for (let at = 0; at < tokens.length; at++) {
    if (!free(at)) continue;
    const word = tokens[at].word;
    const two = free(at + 1) ? `${word} ${tokens[at + 1].word}` : "";
    if (two && NOT_A_TYPE.has(two)) {
      at++;
      continue;
    }
    const before = at > 0 ? tokens[at - 1].word : "";
    const phrase = two ? TYPE_PHRASES.find(([name]) => name === two) : undefined;
    let candidate: TypeCandidate | null = null;
    if (phrase) {
      candidate = { start: at, end: at + 2, ...phrase[1] };
    } else {
      const extension = /.\.([a-z0-9]+)$/.exec(word);
      const known = extension ? TYPE_WORDS.get(extension[1]) : undefined;
      if (known) candidate = { start: at, end: at + 1, fileType: known.fileType, keep: true };
      else {
        const single = TYPE_WORDS.get(word);
        if (single) candidate = { start: at, end: at + 1, ...single };
      }
    }
    if (!candidate) continue;
    at = candidate.end - 1;
    if (NOT.has(before)) continue;
    if (ignored.has(wordsOf(candidate))) continue;
    found.push(candidate);
  }
  return found;
}

function chooseType(candidates: TypeCandidate[], tokens: Token[]): TypeCandidate | null {
  if (candidates.length === 0) return null;
  const types = new Set(candidates.map((candidate) => candidate.fileType));
  if (types.size === 1) return candidates[0];
  for (let at = 0; at + 1 < candidates.length; at++) {
    const [one, other] = [candidates[at], candidates[at + 1]];
    const between = tokens.slice(one.end, other.start).map((token) => token.word);
    if (one.fileType !== other.fileType && between.length === 1 && /^(and|or|&|\/|plus)$/.test(between[0])) return null;
  }
  for (const candidate of candidates) {
    if (candidate.end < tokens.length && HEAD_OF.has(tokens[candidate.end].word)) return candidate;
  }
  return candidates.find((candidate) => !candidate.keep) ?? candidates[0];
}

function trimEnds(tokens: Token[], taken: boolean[], somethingTaken: boolean) {
  const kept = tokens.map((_, at) => at).filter((at) => !taken[at]);
  let asked = false;
  for (;;) {
    const opener = OPENERS.find((phrase) => phrase.every((part, offset) => offset < kept.length && tokens[kept[offset]].word === part));
    if (!opener) break;
    kept.splice(0, opener.length).forEach((at) => (taken[at] = true));
    asked = true;
  }

  const leading = somethingTaken ? LEADING : asked ? words("a an the my our some any all") : words("my our");
  while (kept.length > (somethingTaken || asked ? 0 : 1)) {
    const word = tokens[kept[0]].word;
    if (hasContent(word) && !leading.has(word)) break;
    taken[kept.shift()!] = true;
  }
  if (taken[tokens.length - 1]) {
    while (kept.length > 0) {
      const word = tokens[kept[kept.length - 1]].word;
      if (hasContent(word) && !TRAILING.has(word)) break;
      taken[kept.pop()!] = true;
    }
  }
}

function leftOver(query: string, tokens: Token[], taken: boolean[]): string {
  let text = "";
  let previous = -1;
  tokens.forEach((token, at) => {
    if (taken[at]) return;
    if (previous >= 0) text += previous === at - 1 ? query.slice(tokens[previous].end, token.start) : " ";
    text += query.slice(token.start, token.end);
    previous = at;
  });
  return text;
}

export function understand(query: string, now: Date, ignored: readonly string[] = []): Understood {
  if (query.trim() === "") return NOTHING;
  const tokens = tokenize(query);
  if (tokens.length === 0) return { ...NOTHING, text: cleanQuery(query) };
  const taken = tokens.map(() => false);
  const ignoredSet = new Set(ignored);
  const wordsOf = (span: Span) => query.slice(tokens[span.start].start, tokens[span.end - 1].end).toLowerCase();

  const dates = readDates(tokens, now, ignoredSet, wordsOf);
  for (const span of dates.spans) {
    for (let at = span.start; at < span.end; at++) taken[at] = true;
    let left = span.start - 1;
    for (; left >= 0 && !taken[left] && LEFT_OF_DATE.has(tokens[left].word); left--) taken[left] = true;
    if (left < span.start - 1 && left >= 0 && /^(i|we)$/.test(tokens[left].word)) {
      taken[left] = true;
      if (left > 0 && /^(that|which)$/.test(tokens[left - 1].word)) taken[left - 1] = true;
    }
  }

  const candidates = typeCandidates(tokens, taken, ignoredSet, wordsOf);
  const type = chooseType(candidates, tokens);
  for (const same of type ? candidates.filter((candidate) => candidate.fileType === type.fileType && !candidate.keep) : []) {
    for (let at = same.start; at < same.end; at++) taken[at] = true;
    for (let at = same.start - 1; at >= 0 && !taken[at] && LEFT_OF_TYPE.has(tokens[at].word); at--) taken[at] = true;
  }
  if (type) {
    let at = type.end;
    const said = /^(i|we)$/.test(tokens[at]?.word ?? "") || (/^(that|which)$/.test(tokens[at]?.word ?? "") && /^(i|we)$/.test(tokens[at + 1]?.word ?? ""));
    if (said && !type.keep) {
      at += /^(i|we)$/.test(tokens[at].word) ? 1 : 2;
      if (/^(took|take|shot|made|recorded|filmed|wrote|saved|downloaded|have|had|got)$/.test(tokens[at]?.word ?? "")) {
        for (let drop = type.end; drop <= at; drop++) taken[drop] = true;
        at++;
      } else at = type.end;
    }
    if (!type.keep && at < tokens.length && !taken[at] && RIGHT_OF_TYPE.has(tokens[at].word)) taken[at] = true;
    else if (type.keep && at < tokens.length && !taken[at] && /^(file|files)$/.test(tokens[at].word)) taken[at] = true;
  }

  const range = dates.range;
  const dateSpans = [...dates.spans];
  if (type && dates.spans.length === 0) {
    for (const at of [type.start - 1, type.end]) {
      if (at < 0 || at >= tokens.length || taken[at]) continue;
      const month = /^[a-z]+$/.test(tokens[at].word) && tokens[at].word.length > 3 ? MONTHS.get(tokens[at].word) : undefined;
      if (month === undefined || WORD_MONTHS.has(tokens[at].word) || ignoredSet.has(tokens[at].word)) continue;
      const recent = new DateReader(tokens, now).recentMonth(month);
      range.from = recent;
      range.to = recent;
      taken[at] = true;
      dateSpans.push({ start: at, end: at + 1 });
      break;
    }
  }

  const removed = taken.some(Boolean);
  trimEnds(tokens, taken, removed);
  let text = leftOver(query, tokens, taken);
  if (text === "" && !type && range.from === null && range.to === null) text = query;
  if (!taken.some(Boolean)) text = query;

  return {
    fileType: type ? type.fileType : null,
    from: range.from !== null ? period(range.from) : null,
    to: range.to !== null ? period(range.to) : null,
    text: cleanQuery(text),
    fileTypeWords: type ? [wordsOf(type)] : [],
    dateWords: dateSpans.map(wordsOf),
  };
}

export interface Chosen {
  fileType?: { value: Category | null; words: string[] };
  dates?: { from: Month | null; to: Month | null; words: string[] };
}

const isSaidIn = (said: string[], words: string[]) => said.every((word) => words.includes(word));

export function understandChosen(query: string, now: Date, chosen: Chosen): Understood {
  const type = chosen.fileType;
  const dates = chosen.dates;
  const misread = [
    ...(type && type.value === null ? type.words : []),
    ...(dates && dates.from === null && dates.to === null ? dates.words : []),
  ];
  const read = understand(query, now, misread);
  const result = { ...read };
  if (type && isSaidIn(read.fileTypeWords, type.words)) result.fileType = type.value;
  if (dates && isSaidIn(read.dateWords, dates.words)) {
    result.from = dates.from !== null ? period(dates.from) : null;
    result.to = dates.to !== null ? period(dates.to) : null;
  }
  return result;
}

export function keepChosen(chosen: Chosen, query: string): Chosen {
  const lower = query.toLowerCase();
  const there = (entry?: { words: string[] }) => !entry || entry.words.every((word) => lower.includes(word));
  if (there(chosen.fileType) && there(chosen.dates)) return chosen;
  return { fileType: there(chosen.fileType) ? chosen.fileType : undefined, dates: there(chosen.dates) ? chosen.dates : undefined };
}

function everyReading(query: string, now: Date, words: string[], read: (understood: Understood) => string[]): string[] {
  const all = [...words];
  for (;;) {
    const more = read(understand(query, now, all)).filter((word) => !all.includes(word));
    if (more.length === 0) return all;
    all.push(...more);
  }
}

export function chooseFileType(chosen: Chosen, query: string, now: Date, value: Category | null): Chosen {
  const words = [...(chosen.fileType?.words ?? []), ...understand(query, now).fileTypeWords];
  return {
    ...chosen,
    fileType: { value, words: value === null ? everyReading(query, now, words, (read) => read.fileTypeWords) : words },
  };
}

export function chooseDateEnd(chosen: Chosen, query: string, now: Date, end: "from" | "to", month: Month | null): Chosen {
  const current = understandChosen(query, now, chosen);
  let from = end === "from" ? month : (current.from?.month ?? null);
  let to = end === "to" ? month : (current.to?.month ?? null);
  if (from !== null && to !== null && from > to) {
    if (end === "from") to = from;
    else from = to;
  }
  const words = [...(chosen.dates?.words ?? []), ...understand(query, now).dateWords];
  const open = from === null && to === null;
  return { ...chosen, dates: { from, to, words: open ? everyReading(query, now, words, (read) => read.dateWords) : words } };
}
