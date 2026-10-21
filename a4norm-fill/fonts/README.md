# The answers' fonts

`Arimo-Regular.ttf`: Arimo, by the Arimo Project Authors, from
<https://github.com/googlefonts/Arimo> at commit `4a6255f269916ae7ad3fc2706b0935e7621396b8`
(`fonts/ttf/Arimo-Regular.ttf`, version 1.33), unchanged.
sha256 `41b22bc8f0b51f932825d37bc55b5eb6ba67dfe599a626e4aff2b43b624f9f8c`, 478 712 bytes.

It is under the **SIL Open Font License 1.1** (`OFL.txt`), not the repository's MIT licence.
The OFL allows embedding the font, and subsets of it, in documents.

Why Arimo: its metrics are Arial's (and so Helvetica's), and it has Latin Extended,
Cyrillic and Greek. The names A4Norm Forms' users write ("Yılmaz", "Şahin", "Łukasz",
"Đorđević", "Ștefan", "Müller-Straße") are set as written. The module carries it
(`include_bytes!`), so nothing is fetched at run time. A filled PDF embeds only the glyphs its
answers use.

`Arimo-Bold.ttf`: Arimo Bold, by the same authors under the same licence (its name table:
"Copyright 2020 The Arimo Project Authors (https://github.com/googlefonts/arimo)", "Version
1.341", the OFL 1.1 notice), unchanged, a static instance with all 3 241 glyphs.
sha256 `ec96ac9fdd94766f66f6140c1375bf011577558f88d43f76b3b59307d2a6bca3`, 318 844 bytes.
It was taken from a local copy of the released font (a web build's assets), not fetched
from the commit above; its vertical metrics are Regular's (cap height 1409, descent 434 in
2048 units) and its widths Arial Bold's. It sets a field styled bold (`style.bold`); a PDF
embeds it, as a subset, only when some text is bold.
