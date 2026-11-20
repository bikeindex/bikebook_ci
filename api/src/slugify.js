// bikebook's README's, which names its files, so a name finds the slug its file is under
const FOLDS = { ß: "ss", æ: "ae", œ: "oe", ø: "o", đ: "d", ð: "d", ł: "l", þ: "th" };
const ENTITIES = { amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'", apos: "'" };

export function slugify(string) {
  const s = String(string ?? "").replace(/\([^)]*\)/g, "").replaceAll("%20", " ")
    .replace(/&(amp|lt|gt|quot|#39|apos);/g, (_, e) => ENTITIES[e])
    .toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "")
    .replace(/[ßæœøđðłþ]/g, (c) => FOLDS[c]);
  return (s.replaceAll("&", " and ").replaceAll("+", " plus ").match(/[a-z0-9]+/g) || []).join("_");
}

const COMPANY_FILLER = /\sco(\.|mpany)|\s(frame)?works/gi;
const MANUFACTURER_FILLER = /\s(e(lectric)?.?)?((bi)?cycles?|bikes?)|\sco(\.|mpany)|\s(frame)?works/gi;

export function manufacturerSlug(name) {
  const slug = slugify(String(name ?? "").replace(MANUFACTURER_FILLER, " "));
  return /\D/.test(slug) ? slug : slugify(String(name ?? "").replace(COMPANY_FILLER, " "));
}
