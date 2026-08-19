; ModuleID = 'hello'
source_filename = "hello.rx"
target triple = "x86_64-pc-windows-msvc"

@.str.0 = private unnamed_addr constant [13 x i8] c"hello, rurix\00"
declare i32 @puts(ptr)

define i32 @main() !dbg !6 {
start:
  %l1 = alloca ptr
  br label %bb0
bb0:
  store ptr @.str.0, ptr %l1, !dbg !7
  %t1 = load ptr, ptr %l1, !dbg !8
  %t2 = call i32 @puts(ptr %t1), !dbg !8
  br label %bb1, !dbg !8
bb1:
  ret i32 0, !dbg !9
}


!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}
!0 = distinct !DICompileUnit(language: DW_LANG_C99, file: !1, producer: "rurixc", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug)
!1 = !DIFile(filename: "hello.rx", directory: "D:\\游戏引擎\\tests\\fixtures\\f4")
!2 = !{i32 2, !"CodeView", i32 1}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !DISubroutineType(types: !5)
!5 = !{null}
!6 = distinct !DISubprogram(name: "main", linkageName: "main", scope: !1, file: !1, line: 1, type: !4, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!7 = !DILocation(line: 2, column: 20, scope: !6)
!8 = !DILocation(line: 3, column: 5, scope: !6)
!9 = !DILocation(line: 1, column: 11, scope: !6)
