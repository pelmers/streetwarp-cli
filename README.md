# Hyperlapse streetview images along GPX tracks

![](res/example.gif)

### Prerequisites
1. **Install [ffmpeg](https://ffmpeg.org/download.html)** (or build it with h264 encoding)
2. Get a **Google Maps API key** [from here](https://developers.google.com/maps/documentation/streetview/)
3. Activate the Streetview static API from [this page](https://console.cloud.google.com/apis/library/street-view-image-backend.googleapis.com)
4. Record your API key from [this page](https://console.cloud.google.com/apis/credentials) of the console

### API Usage notes
Some back of the napkin estimations:
  - Google gives you $200 of free credit every month
  - The [Streetview static API](https://developers.google.com/maps/documentation/streetview/) costs $0.007 per frame
  - In the densest areas we can download about 200 images per mile
  - This lets you render up to **300** miles per month for free
   
To **avoid hitting your API quota**, pass in the `--dry-run` option!

### Usage
`cargo run -- --help`

Included in this repo are some gpx files you can use to play around with.