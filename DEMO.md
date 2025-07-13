# 対応するMarkdown記法

`Markdown`には様々な書き方があります。

ここでは、本拡張機能が対応するMarkdown記法を紹介します。

## ヘッダー

### 書き方

```
# h1
## h2
### h3
#### h4
##### h5
###### h6
```

### 結果

# h1
## h2
### h3
#### h4
##### h5
###### h6

## 数式

### 書き方

```plaintext
文中に挿入するなら$e^{jx} = \cos x + j \sin x$のように。
$$
\begin{aligned}
    \cos x &= \sum_{n=0}^{\infty} \frac{(-1)^{n}}{(2n)!}x^{2n} \\
    \sin x &= \sum_{n=0}^{\infty} \frac{(-1)^{n}}{(2n+1)!}x^{2n+1}
\end{aligned}
$$
```

### 結果

文中に挿入するなら$e^{jx} = \cos x + j \sin x$のように。
$$
\begin{aligned}
    \cos x &= \sum_{n=0}^{\infty} \frac{(-1)^{n}}{(2n)!}x^{2n} \\
    \sin x &= \sum_{n=0}^{\infty} \frac{(-1)^{n}}{(2n+1)!}x^{2n+1}
\end{aligned}
$$

## コードブロック

### 書き方

```js showLineNumbers title="add.js" {3}
const add = (a, b) => a + b
add(2, 3) // 5
add(4, 5) // 9
```

### 結果

```js showLineNumbers title="add.js" {3}
const add = (a, b) => a + b
add(2, 3) // 5
add(4, 5) // 9
```

## iframe生成

### 書き方

```plaintext
https://www.youtube.com/watch?v=G1W3aroArqY

https://youtu.be/G1W3aroArqY?si=ikN7z2VwhoprUT8M

https://youtu.be/G1W3aroArqY
```

### 結果
https://www.youtube.com/watch?v=G1W3aroArqY

https://youtu.be/G1W3aroArqY?si=ikN7z2VwhoprUT8M

https://youtu.be/G1W3aroArqY

## Admonition・Alert記法

:::::tabs
  ::::tab[info]

    ### 書き方

    ```plaintext
    :::info
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::

    :::info[Special Title]
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::
    ```

    ### 結果

    :::info
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::

    :::info[Special Title]
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::

  ::::

  ::::tab[tip]

    ### 書き方

    ```plaintext
    :::tip
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::

    :::tip[Special Title]
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::
    ```

    ### 結果

    :::tip
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::

    :::tip[Special Title]
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::

  ::::

  ::::tab[warning]

    ### 書き方

    ```plaintext
    :::warning
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::

    :::warning[Special Title]
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::
    ```

    ### 結果

    :::warning
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::

    :::warning[Special Title]
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::

  ::::

  ::::tab[danger]

    ### 書き方

    ```plaintext
    :::danger
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::

    :::danger[Special Title]
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::
    ```

    ### 結果

    :::danger
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::

    :::danger[Special Title]
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
    :::

  ::::

:::::

## タブ記法

### 書き方

```plaintext
::::tabs

  :::tab[タブ1のタイトル]
  タブ1の内容がここに表示されます。
  複数行でもOKです。
  - リストも
  - 書けます

  **Markdownの書式**も使えます。
  :::

  :::tab[タブ2のタイトル]
  タブ2の内容がここに表示されます。
  複数行でもOKです。
  - リストも
  - 書けます

  **Markdownの書式**も使えます。
  :::

::::
```

### 結果

::::tabs

  :::tab[タブ1のタイトル]
  タブ1の内容がここに表示されます。
  複数行でもOKです。
  - リストも
  - 書けます

  **Markdownの書式**も使えます。
  :::

  :::tab[タブ2のタイトル]
  タブ2の内容がここに表示されます。
  複数行でもOKです。
  - リストも
  - 書けます

  **Markdownの書式**も使えます。
  :::

::::

## 折り畳み

:::::tabs

  ::::tab[推奨]

  ### 書き方

  ```plaintext
  :::details
    折りたたまれていた内容がここに表示されます。
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
  :::

  :::details[タイトル付]
    折りたたまれていた内容がここに表示されます。
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
  :::
  ```

  ### 結果

  :::details
    折りたたまれていた内容がここに表示されます。
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
  :::

  :::details[タイトル付]
    折りたたまれていた内容がここに表示されます。
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
  :::

  ::::

  :::tab[HTML版]

  ### 書き方

  ```plaintext
  <details>
    <summary>ここをクリックして展開</summary>

    折りたたまれていた内容がここに表示されます。
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
  </details>
  ```

  ### 結果

  <details>
    <summary>ここをクリックして展開</summary>

    折りたたまれていた内容がここに表示されます。
    複数行でもOKです。
    - リストも
    - 書けます

    **Markdownの書式**も使えます。
  </details>

  :::

:::::

## Mermaid

### 書き方

````plaintext
```mermaid
sequenceDiagram
    participant Alice
    participant Bob
    Alice->>John: Hello John, how are you?
    loop HealthCheck
        John->>John: Fight against hypochondria
    end
    Note right of John: Rational thoughts <br/>prevail!
    John-->>Alice: Great!
    John->>Bob: How about you?
    Bob-->>John: Jolly good!
```
````

### 結果

```mermaid
sequenceDiagram
    participant Alice
    participant Bob
    Alice->>John: Hello John, how are you?
    loop HealthCheck
        John->>John: Fight against hypochondria
    end
    Note right of John: Rational thoughts <br/>prevail!
    John-->>Alice: Great!
    John->>Bob: How about you?
    Bob-->>John: Jolly good!
```

## ABC.js

### 書き方

````plaintext
```abc
X: 1
T: Fontaine
M: 6/8
L: 1/8
K: Em
Q: 170
%%MIDI gchord fccfcc
"Em"e3 "Fm/E"d2 c | "Em"B3-BEF | "Am"GAB AGE | "B+"G3 "B7"^F3 |
"Em"ze2 "Fm"d2 c | "Em"B3-"A9/C#"BEF | "Am"GAB "B7"AGF | "Em"E3 zEF|
"Am7"GFG "D7"A2d | "Gmaj7"B3 zEF| "Am7"GFG "D7"A2d | "Gmaj7"B3 zB^c|
"Bm"d^cB"F#m"c2F | "G"BAG"D"A2D | "Em"GFE"F#+"D2^C |"Bm"B,6|
```
````

### 結果

```abc
X: 1
T: Fontaine
M: 6/8
L: 1/8
K: Em
Q: 170
%%MIDI gchord fccfcc
"Em"e3 "Fm/E"d2 c | "Em"B3-BEF | "Am"GAB AGE | "B+"G3 "B7"^F3 |
"Em"ze2 "Fm"d2 c | "Em"B3-"A9/C#"BEF | "Am"GAB "B7"AGF | "Em"E3 zEF|
"Am7"GFG "D7"A2d | "Gmaj7"B3 zEF| "Am7"GFG "D7"A2d | "Gmaj7"B3 zB^c|
"Bm"d^cB"F#m"c2F | "G"BAG"D"A2D | "Em"GFE"F#+"D2^C |"Bm"B,6|
```