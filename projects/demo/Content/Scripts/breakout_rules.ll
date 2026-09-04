; ModuleID = 'breakout_rules'
source_filename = "breakout_rules.rx"
target triple = "x86_64-pc-windows-msvc"


define i32 @main() !dbg !6 {
start:
  br label %bb0
bb0:
  ret i32 0, !dbg !7
}


!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}
!0 = distinct !DICompileUnit(language: DW_LANG_C99, file: !1, producer: "rurixc", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug)
!1 = !DIFile(filename: "breakout_rules.rx", directory: "D:\\游戏引擎\\projects\\demo\\Content\\Scripts")
!2 = !{i32 2, !"CodeView", i32 1}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !DISubroutineType(types: !5)
!5 = !{null}
!6 = distinct !DISubprogram(name: "main", linkageName: "main", scope: !1, file: !1, line: 4, type: !4, scopeLine: 4, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!7 = !DILocation(line: 4, column: 11, scope: !6)
