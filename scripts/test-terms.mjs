// 기록 화면 검색어 나누기·찾은 곳 표시 시험: node scripts/test-terms.mjs
// (Node 22.6+ 의 TypeScript 형식 제거로 src/lib/terms.ts 를 바로 읽는다)
import { parseTerms, splitHits } from "../src/lib/terms.ts";

let failed = 0;
const eq = (name, got, want) => {
  const ok = JSON.stringify(got) === JSON.stringify(want);
  if (!ok) failed++;
  console.log(`${ok ? "OK " : "NG "} ${name}${ok ? "" : `\n    받음: ${JSON.stringify(got)}\n    기대: ${JSON.stringify(want)}`}`);
};

eq("낱말 하나", parseTerms("홍길동", "words"), ["홍길동"]);
eq("낱말 둘: 띄어쓰기로 나눔", parseTerms("홍길동  상담", "words"), ["홍길동", "상담"]);
eq("따옴표는 구 그대로", parseTerms('"보호자 연락" 홍길동', "words"), ["보호자 연락", "홍길동"]);
eq("둥근 따옴표도", parseTerms("“보호자   연락”", "words"), ["보호자 연락"]);
eq("같은 낱말은 한 번", parseTerms("홍길동 홍길동", "words"), ["홍길동"]);
eq("구 그대로 방식: 통째로", parseTerms(" 보호자 연락 ", "phrase"), ["보호자 연락"]);
eq("빈 검색", parseTerms("   ", "words"), []);
eq(
  "찾은 곳 표시 (긴 낱말 먼저, 대소문자 무시)",
  splitHits("홍길동 보호자 PPT 상담", ["보호자", "ppt", "홍길동"]).map((p) => (p.hit ? `[${p.text}]` : p.text)).join(""),
  "[홍길동] [보호자] [PPT] 상담",
);
eq("특수 문자도 글자 그대로", splitHits("자료 100% (1차)", ["100%", "(1차)"]).map((p) => (p.hit ? `[${p.text}]` : p.text)).join(""), "자료 [100%] [(1차)]");

console.log(failed ? `결과: ${failed}개 실패` : "결과: 모두 통과");
process.exit(failed ? 1 : 0);
