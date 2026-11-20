// bikebook's README's, which names its model files, so a model's name finds the slug its file is under
const FOLDS = { ß: "ss", æ: "ae", œ: "oe", ø: "o", đ: "d", ð: "d", ł: "l", þ: "th" };
const ENTITIES = { amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'", apos: "'" };

export function slugify(string) {
  const s = String(string ?? "").replace(/\([^)]*\)/g, "").replaceAll("%20", " ")
    .replace(/&(amp|lt|gt|quot|#39|apos);/g, (_, e) => ENTITIES[e])
    .toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "")
    .replace(/[ßæœøđðłþ]/g, (c) => FOLDS[c]);
  return (s.replaceAll("&", " and ").replaceAll("+", " plus ").match(/[a-z0-9]+/g) || []).join("_");
}
