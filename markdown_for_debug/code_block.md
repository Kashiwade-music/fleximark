```js
document.body.addEventListener('click', () => {
  const colors = ['blue', 'green', 'pink', 'yellow'];
  const randomColor = colors[Math.floor(Math.random() * colors.length)];
  document.body.style.backgroundColor = randomColor;
});
```

```js title="color.js"
document.body.addEventListener('click', () => {
  const colors = ['blue', 'green', 'pink', 'yellow'];
  const randomColor = colors[Math.floor(Math.random() * colors.length)];
  document.body.style.backgroundColor = randomColor;
});
```

```js title="color.js" showLineNumbers
document.body.addEventListener('click', () => {
  const colors = ['blue', 'green', 'pink', 'yellow'];
  const randomColor = colors[Math.floor(Math.random() * colors.length)];
  document.body.style.backgroundColor = randomColor;
});
```

```js title="color.js" showLineNumbers=5
document.body.addEventListener('click', () => {
  const colors = ['blue', 'green', 'pink', 'yellow'];
  const randomColor = colors[Math.floor(Math.random() * colors.length)];
  document.body.style.backgroundColor = randomColor;
});
```

```js title="color.js" showLineNumbers=10
document.body.addEventListener('click', () => {
  const colors = ['blue', 'green', 'pink', 'yellow'];
  const randomColor = colors[Math.floor(Math.random() * colors.length)];
  document.body.style.backgroundColor = randomColor;
});
```

```js title="color.js" showLineNumbers=100
document.body.addEventListener('click', () => {
  const colors = ['blue', 'green', 'pink', 'yellow'];
  const randomColor = colors[Math.floor(Math.random() * colors.length)];
  document.body.style.backgroundColor = randomColor;
});
```

```js title="color.js" showLineNumbers {1,3-5}
document.body.addEventListener('click', () => {
  const colors = ['blue', 'green', 'pink', 'yellow'];
  const randomColor = colors[Math.floor(Math.random() * colors.length)];
  document.body.style.backgroundColor = randomColor;
});
```

```js title="color.js"
// [!code highlight]
document.body.addEventListener('click', () => {
  const colors = ['blue', 'green', 'pink', 'yellow'];
  // [!code highlight:3]
  const randomColor = colors[Math.floor(Math.random() * colors.length)];
  document.body.style.backgroundColor = randomColor;
});
```

```js title="color.js" /colors/
document.body.addEventListener('click', () => {
  const colors = ['blue', 'green', 'pink', 'yellow'];
  const randomColor = colors[Math.floor(Math.random() * colors.length)];
  document.body.style.backgroundColor = randomColor;
});
```

```js title="color.js"
document.body.addEventListener('click', () => {
  const colors = ['blue', 'green', 'pink', 'yellow']; // [!code --]
  // [!code ++:7]
  const colors = [
    'blue', 
    'green', 
    'pink', 
    'yellow', 
    'red'
  ];
  const randomColor = colors[Math.floor(Math.random() * colors.length)];
  document.body.style.backgroundColor = randomColor;
});
```

```js title="color.js" showLineNumbers
document.body.addEventListener('click', () => {
  const colors = ['blue', 'green', 'pink', 'yellow']; // [!code --]
  // [!code ++:7]
  const colors = [
    'blue', 
    'green', 
    'pink', 
    'yellow', 
    'red'
  ];
  const randomColor = colors[Math.floor(Math.random() * colors.length)];
  document.body.style.backgroundColor = randomColor;
});
```

