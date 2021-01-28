/*
  The common code for both visibility and composition stages that
  calculates the appropriate position based on a tile number

  Austin Shafer - 2021
*/

/* The width of a square tile of pixels in the screen */
#define TILESIZE 16

/* the position/size/damage of our windows */
layout(binding = 1) buffer tiles
{
	int width;
	int height;
	int active_tiles[];
};


ivec2 get_location_for_wg() {
	/*
	   - Get the tile for this wg from the list we initialized.
	   This tells us the base address that we are working on.
	 */
	int tile = active_tiles[gl_WorkGroupID.x];

	/*
	   - Mod the tile address by our resolution's width (in tiles) to get the
	   depth into a row of the framebuffer.
	   - Dividing by the width gives use the number of rows into the
	   image.
	   - Multiply them both by the tilesize to take us from the tile-grid
	   coordinate space to the pixel coordinate space
	 */
	int tiles_width = (width / TILESIZE) + 1;
	ivec2 tile_base = ivec2(mod(tile, tiles_width) * TILESIZE, (tile / tiles_width) * TILESIZE);
	/* Now index into the tile based on this invocation */
	return ivec2(tile_base.x + gl_LocalInvocationID.x,
			tile_base.y + gl_LocalInvocationID.y);
}
