# التوثيق المعماري لمشروع `ARZ-Excalibur` (Adobe Premiere Pro)

## المعمارية الهندسية الهجينة (`Rust Native Core` + `CEP Thin Bridge`)

صُمم **`ARZ-Excalibur`** بلغة **Rust** لتوفير سرعة استجابة أقل من `2ms` واستهلاك ذاكرة منخفض جداً مقارنة بالإضافات التقليدية المعتمدة بالكامل على المتصفحات المدمجة:

1. **النواة الأصلية (`ARZ-Excalibur.exe` — Rust):**
   - **`src/win32_input.rs`:** خطاف لوحة مفاتيح منخفض المستوى (`WH_KEYBOARD_LL`) مقيد بنافذة Premiere Pro النشطة، مع محرك نقر مباشر على شريط القوائم (`GetMenu` / `WM_COMMAND`) ومحاكاة الاختصارات (`SendInput`).
   - **`src/preview_renderer.rs`:** محرك معاينة بصرية بسرعة `60 FPS` يسحب الفريم الفعلي تحت المؤشر من التايملاين ويعرض تأثير الفلاتر والانتقالات حياً داخل شبكة مربعات ثلاثية الأعمدة (`3-Column Grid`).
   - **`src/premiere_presets.rs`:** محلل XML فائق السرعة (`quick-xml`) لملفات البريسيت `.prfpset` مع مراقبة التعديلات اللحظية (`notify`).
   - **`src/math_expr.rs` & `src/macro_engine.rs`:** حاسبة القيم النسبية (`+`, `-`, `*`, `/`, `%`) ومشغل أوامر الماكرو المتسلسلة.
2. **جسر ولوحة التحكم داخل Premiere Pro (`premiere-bridge/com.arz.excalibur.bridge`):**
   - **`client/bridge.js` & `host/arz_premiere.jsx`:** يدير تشغيل وإغلاق المحرك تلقائياً مع فتح وإغلاق Premiere Pro، وينفذ الأوامر عبر `Official DOM` و`QE DOM` على منفذ محلي مغلق (`127.0.0.1:49872`).
   - **`client/settings.html`:** لوحة تحكم متكاملة بتبويبين (`Commands & Macros` و`General Settings`) بهوية **Noir & Brass** لتخصيص الاختصارات وبناء الماكرو وإظهار أو إخفاء الأوامر.
