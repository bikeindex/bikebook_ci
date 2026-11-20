// Bike Index's Slugifyer, which slugs its manufacturers. Ruby's \s, strip and I18n.transliterate are ASCII-only, so
// these are too: any other character transliterates to "?", which the slug drops
const TRANSLITERATIONS = Object.fromEntries(
  "×x ßss àa áa âa ãa äa åa æae çc èe ée êe ëe ìi íi îi ïi ðd ñn òo óo ôo õo öo øo ùu úu ûu üu ýy þth ÿy āa ăa ąa ćc ĉc ċc čc ďd đd ēe ĕe ėe ęe ěe ĝg ğg ġg ģg ĥh ħh ĩi īi ĭi įi ıi ĳij ĵj ķk ĸk ĺl ļl ľl ŀl łl ńn ņn ňn ŉ'n ŋng ōo ŏo őo œoe ŕr ŗr řr śs ŝs şs šs ţt ťt ŧt ũu ūu ŭu ůu űu ųu ŵw ŷy źz żz žz ǫo ǭo"
    .split(" ")
    .map((pair) => [pair[0], pair.slice(1)]),
);

const strip = (string) => string.replace(/^[\0\t-\r ]+|[\0\t-\r ]+$/g, "");
const removeParentheses = (string) => String(string ?? "").replace(/\([^)]*\)/g, "");
const transliterate = (string) =>
  strip(string.toLowerCase().replace(/[^\x00-\x7f]/gu, (character) => TRANSLITERATIONS[character] ?? "?"));

/** `Binxtils::InputNormalizer.string`, which Bike Index's names and lookups go through first */
export const normalize = (string) => strip(string.replaceAll("\0", "")).replace(/[\t-\r ]+/g, " ");

export const slugify = (string) =>
  transliterate(removeParentheses(string))
    .replaceAll("%20", " ")
    .replace(/[\t-\r ]/g, "-")
    .replaceAll("-&-", "-amp-")
    .replace(/[^A-Za-z0-9_-]+/g, "");

const bookSlug = (string) =>
  strip(
    transliterate(string)
      .replace(/%20/gi, " ")
      .replace(/[\t-\r ](e(lectric)?[^\n]?)?((bi)?cycles?|bikes?)/gi, " ")
      .replace(/\+/g, "plus")
      .replace(/&/g, "amp")
      .replace(/[^A-Za-z0-9]/g, " "),
  ).replace(/[\t-\r ]+/g, "_");

export function legacyManufacturer(string) {
  const name = removeParentheses(string)
    .replace(/[\t-\r ]co(\.|mpany)/gi, " ")
    .replace(/[\t-\r ](frame)?works/gi, " ");
  const slug = bookSlug(name.replace(/accell/gi, ""));
  return /\D/.test(slug) ? slug : slugify(name).replaceAll("-", "_");
}
