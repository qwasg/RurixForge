const OUTPUT_LINES = [
  '$ cargo test --workspace',
  '   Compiling rurixc v1.0.0 (H:\\rurix\\src\\rurixc)',
  '   Compiling rurix-rt v1.0.0 (H:\\rurix\\src\\rurix-rt)',
  '    Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 12s',
  '     Running unittests src\\lib.rs (target\\debug\\deps\\rurixc-3f7a9c1e.exe)',
  '',
  'test result: ok. 350 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out',
  '',
];

/** Terminal 视图:黑底等宽假终端(静态输出 + 闪烁光标)。 */
export default function TerminalView() {
  return (
    <div className="h-full overflow-auto bg-[#1e1e1e] p-3 font-mono text-xs leading-5">
      {OUTPUT_LINES.map((line, i) => (
        <div key={i} className="whitespace-pre-wrap text-[#8a8a8a]">
          {line || ' '}
        </div>
      ))}
      <div className="text-[#c8c8c8]">
        ${' '}
        <span className="inline-block h-[13px] w-[7px] animate-pulse bg-[#c8c8c8] align-text-bottom" />
      </div>
    </div>
  );
}
