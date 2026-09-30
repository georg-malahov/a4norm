# The answers' font

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
